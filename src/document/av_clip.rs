use serde::{Deserialize, Serialize};
use uuid::Uuid;

const AUDIO_EXTS: &[&str] = &["mp3", "wav", "aac", "m4a", "flac", "ogg", "opus", "wma"];
const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp", "tif", "tiff"];
const VIDEO_EXTS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "webm", "m4v", "wmv", "flv", "mpeg", "mpg", "ts",
];

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
    /// Live link to document object(s). When set, the track re-rasterizes when those nodes change.
    #[serde(default)]
    pub source_node_ids: Vec<Uuid>,
    /// Muted clips are skipped by preview and export (audition without delete).
    #[serde(default)]
    pub muted: bool,
    /// Locked clips refuse move/trim/delete/split (must unlock first).
    #[serde(default)]
    pub locked: bool,
}

impl AvClip {
    pub fn new_from_media(
        name: impl Into<String>,
        path: impl Into<String>,
        timeline_start: f32,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            media_path: path.into(),
            video_start_offset: 0.0,
            video_play_length: 3600.0,
            video_timeline_start: timeline_start,
            media_source_duration: None,
            track_row: 0,
            source_node_ids: Vec::new(),
            muted: false,
            locked: false,
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
            source_node_ids: Vec::new(),
            muted: false,
            locked: false,
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
            source_node_ids: Vec::new(),
            muted: false,
            locked: false,
        }
    }

    pub fn is_object_linked(&self) -> bool {
        !self.source_node_ids.is_empty()
    }

    fn path_ext(path: &str) -> String {
        std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
    }

    pub fn path_is_audio_only(path: &str) -> bool {
        if path.is_empty() {
            return false;
        }
        let ext = Self::path_ext(path);
        AUDIO_EXTS.iter().any(|a| *a == ext)
    }

    /// Static / animated image files (PNG, GIF, WebP, …) — shown on the Video queue.
    pub fn path_is_still_image(path: &str) -> bool {
        if path.is_empty() {
            return false;
        }
        let ext = Self::path_ext(path);
        IMAGE_EXTS.iter().any(|a| *a == ext)
    }

    pub fn path_is_video_container(path: &str) -> bool {
        if path.is_empty() {
            return false;
        }
        let ext = Self::path_ext(path);
        VIDEO_EXTS.iter().any(|a| *a == ext)
    }

    /// Visual media for the Video layer queue (video file or image).
    pub fn path_is_visual_media(path: &str) -> bool {
        Self::path_is_video_container(path) || Self::path_is_still_image(path)
    }

    pub fn is_audio_only(&self) -> bool {
        Self::path_is_audio_only(&self.media_path)
    }

    pub fn is_still_image(&self) -> bool {
        Self::path_is_still_image(&self.media_path)
    }

    /// Compatible with a Video-role layer (video or image, not pure audio).
    pub fn path_fits_video_role(path: &str) -> bool {
        Self::path_is_visual_media(path)
    }

    pub fn path_fits_audio_role(path: &str) -> bool {
        Self::path_is_audio_only(path)
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
            c.track_row == row
                && ranges_overlap(
                    start_sec,
                    end_sec,
                    c.video_timeline_start,
                    c.timeline_end_secs(),
                )
        });
        let music_overlap = music_clips.iter().any(|c| {
            c.track_row == row
                && ranges_overlap(start_sec, end_sec, c.timeline_start_sec, c.end_sec())
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

/// Snap a timeline time to the nearest candidate within `threshold`.
/// Candidates: 0, every clip start/end on any row. Returns snapped time.
/// Pure function (no document mutation) — UI drags and scripts share it.
pub fn snap_time(t: f32, clips: &[AvClip], threshold: f32) -> f32 {
    snap_time_with_markers(t, clips, &[], threshold)
}

/// [`snap_time`] plus timeline-marker candidates.
pub fn snap_time_with_markers(
    t: f32,
    clips: &[AvClip],
    markers: &[super::TimelineMarker],
    threshold: f32,
) -> f32 {
    if !t.is_finite() || threshold <= 0.0 {
        return t;
    }
    let mut best = t;
    let mut best_dist = threshold;
    let mut consider = |c: f32| {
        let d = (t - c).abs();
        if d < best_dist {
            best_dist = d;
            best = c;
        }
    };
    consider(0.0);
    for clip in clips {
        consider(clip.video_timeline_start);
        consider(clip.timeline_end_secs());
    }
    for m in markers {
        consider(m.time_sec);
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip_at(start: f32, len: f32) -> AvClip {
        AvClip {
            id: Uuid::new_v4(),
            name: "c".into(),
            media_path: "m.mp4".into(),
            video_start_offset: 0.0,
            video_play_length: len,
            video_timeline_start: start,
            media_source_duration: None,
            track_row: 0,
            source_node_ids: vec![],
            muted: false,
            locked: false,
        }
    }

    #[test]
    fn snap_prefers_nearest_edge() {
        let clips = vec![clip_at(5.0, 10.0)];
        assert_eq!(snap_time(5.2, &clips, 0.5), 5.0);
        assert_eq!(snap_time(14.8, &clips, 0.5), 15.0);
        assert_eq!(snap_time(7.0, &clips, 0.5), 7.0);
        assert_eq!(snap_time(0.2, &clips, 0.5), 0.0);
        assert_eq!(snap_time(f32::NAN, &clips, 0.5).is_finite(), false);
    }

    #[test]
    fn snap_considers_markers() {
        use super::super::TimelineMarker;
        let clips = vec![clip_at(5.0, 10.0)];
        let markers = vec![TimelineMarker::new("Intro", 20.0)];
        // Marker wins over clip end (15.0) when closer.
        assert_eq!(
            snap_time_with_markers(19.8, &clips, &markers, 0.5),
            20.0
        );
        // Out of threshold: untouched.
        assert_eq!(snap_time_with_markers(19.0, &clips, &markers, 0.5), 19.0);
    }
}
