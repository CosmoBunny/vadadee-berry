use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MusicNote {
    pub pitch: u8,
    pub start_tick: u32,
    pub duration_ticks: u32,
    #[serde(default = "default_velocity")]
    pub velocity: u8,
}

fn default_velocity() -> u8 {
    100
}

impl MusicNote {
    pub fn new(pitch: u8, start_tick: u32, duration_ticks: u32) -> Self {
        Self {
            pitch,
            start_tick,
            duration_ticks,
            velocity: 100,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MusicClip {
    pub id: Uuid,
    pub name: String,
    pub timeline_start_sec: f32,
    pub duration_sec: f32,
    /// Sub-track row inside the parent AV layer.
    #[serde(default)]
    pub track_row: u32,
    #[serde(default)]
    pub notes: Vec<MusicNote>,
}

impl MusicClip {
    pub fn new_empty(name: impl Into<String>, timeline_start_sec: f32, duration_sec: f32) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            timeline_start_sec,
            duration_sec,
            track_row: 0,
            notes: Vec::new(),
        }
    }

    pub fn end_sec(&self) -> f32 {
        self.timeline_start_sec + self.duration_sec
    }
}