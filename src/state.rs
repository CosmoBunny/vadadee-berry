//! Split-out application state groups (P1 refactor).
//!
//! Per architecture review §"What I'd change": `VadadeeBerryApp` must stop
//! being one giant state container. State moves here one group at a time —
//! `PlaybackState` first — while methods keep working through `app.playback`.
//! The end goal:
//!
//! ```text
//! EditorState { project-ish document state, selection, ... }
//! UiState { panels, dialogs, inspector focus, ... }
//! PlaybackState { playhead, transport, fps }   <-- this module (done)
//! CollaborationState { ... }
//! ```

/// Animation / timeline transport state.
///
/// Previously seven loose `anim_*` fields on `VadadeeBerryApp` accessed from
/// app.rs, ui.rs, av_ui.rs and node_editor_ui.rs. Grouped so playback can be
/// reasoned about, tested, and eventually driven without the whole app.
#[derive(Debug, Clone)]
pub struct PlaybackState {
    /// Current timeline frame (playhead).
    pub frame: usize,
    /// Whether the timeline is currently playing.
    pub playing: bool,
    /// Wall-clock of the last playback tick (keeps advancing unfocused).
    pub wall_tick: Option<std::time::Instant>,
    /// Absolute play origin: (instant play started, frame at that instant).
    /// Playhead = start_frame + elapsed * fps.
    pub play_origin: Option<(std::time::Instant, usize)>,
    /// Fractional-frame time accumulator for smooth playback.
    pub time_accumulator: f32,
    /// Last frame the animation system applied (change detection).
    pub last_seen_frame: usize,
    /// Timeline frames per second.
    pub fps: u32,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            frame: 0,
            playing: false,
            wall_tick: None,
            play_origin: None,
            time_accumulator: 0.0,
            last_seen_frame: 0,
            fps: 60,
        }
    }
}

impl PlaybackState {
    /// Playhead time in seconds.
    pub fn time_secs(&self) -> f32 {
        self.frame as f32 / self.fps.max(1) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_playhead_at_zero_60fps() {
        let p = PlaybackState::default();
        assert_eq!(p.frame, 0);
        assert!(!p.playing);
        assert_eq!(p.fps, 60);
        assert!((p.time_secs() - 0.0).abs() < f32::EPSILON);
    }
}
