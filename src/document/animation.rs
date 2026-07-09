use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::document::{eval_expr_vars, BezierHandleMode, ExprVars, Fill, NodeId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InterpolationMode {
    Linear,
    Bezier,
}

impl Default for InterpolationMode {
    fn default() -> Self {
        Self::Linear
    }
}

fn default_handle_left() -> (f64, f64) {
    (-5.0, 0.0)
}

fn default_handle_right() -> (f64, f64) {
    (5.0, 0.0)
}

fn default_handle_mode() -> BezierHandleMode {
    BezierHandleMode::Both
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Keyframe {
    pub frame: usize,
    pub value: f64,
    #[serde(default)]
    pub interpolation: InterpolationMode,
    #[serde(default = "default_handle_left")]
    pub handle_left: (f64, f64),
    #[serde(default = "default_handle_right")]
    pub handle_right: (f64, f64),
    #[serde(default = "default_handle_mode")]
    pub handle_mode: BezierHandleMode,
}

fn solve_u(x_target: f64, x1: f64, range: f64) -> f64 {
    if range < 1e-9 {
        return 0.0;
    }
    let mut low = 0.0;
    let mut high = 1.0;
    for _ in 0..24 {
        let u = (low + high) * 0.5;
        let omt = 1.0 - u;
        let x = 2.0 * omt * u * x1 + u * u * range;
        if x < x_target {
            low = u;
        } else {
            high = u;
        }
    }
    (low + high) * 0.5
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct KeyframeTrack {
    pub keyframes: Vec<Keyframe>,
}

impl KeyframeTrack {
    pub fn insert(&mut self, frame: usize, value: f64) {
        if let Some(pos) = self.keyframes.iter().position(|kf| kf.frame == frame) {
            self.keyframes[pos].value = value;
        } else {
            self.keyframes.push(Keyframe {
                frame,
                value,
                interpolation: InterpolationMode::Linear,
                handle_left: (-5.0, 0.0),
                handle_right: (5.0, 0.0),
                handle_mode: BezierHandleMode::Both,
            });
            self.keyframes.sort_by_key(|kf| kf.frame);
        }
    }

    pub fn interpolate(&self, frame: usize) -> Option<f64> {
        if self.keyframes.is_empty() {
            return None;
        }
        if frame <= self.keyframes[0].frame {
            return Some(self.keyframes[0].value);
        }
        let last_idx = self.keyframes.len() - 1;
        if frame >= self.keyframes[last_idx].frame {
            return Some(self.keyframes[last_idx].value);
        }
        for i in 0..last_idx {
            let kf0 = &self.keyframes[i];
            let kf1 = &self.keyframes[i+1];
            if frame >= kf0.frame && frame <= kf1.frame {
                let range = (kf1.frame - kf0.frame) as f64;
                if range < 1e-9 {
                    return Some(kf0.value);
                }
                let t = (frame - kf0.frame) as f64 / range;
                if kf0.interpolation == InterpolationMode::Bezier {
                    let x_target = (frame - kf0.frame) as f64;
                    let x1 = kf0.handle_right.0.clamp(0.0, range);
                    let u = solve_u(x_target, x1, range);
                    
                    let omt = 1.0 - u;
                    let y0 = kf0.value;
                    let y1 = kf0.value + kf0.handle_right.1;
                    let y2 = kf1.value;
                    
                    let val = omt * omt * y0
                        + 2.0 * omt * u * y1
                        + u * u * y2;
                    return Some(val);
                } else {
                    return Some(kf0.value + t * (kf1.value - kf0.value));
                }
            }
        }
        None
    }
}

/// One channel inside a stack animation function (`f(t)` component).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StackAnimChannel {
    /// Track label: pos_x, pos_y, rotation, color_r, geom_0, …
    pub track: String,
    /// Math expression in `t` (0..1) and optional `f` (local frame). Empty = constant `start_value`.
    pub expr: String,
    /// Value at the start of the stack span (also used when `expr` is empty).
    pub start_value: f64,
    /// Last parse/eval error (not serialized).
    #[serde(skip)]
    pub last_error: Option<String>,
}

/// Graph-editor “stack animation function”: formula-driven span that overrides keyframes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StackAnimationFunction {
    pub id: Uuid,
    pub start_frame: usize,
    /// Inclusive span length in frames (min 1). End frame = start + duration.
    pub duration_frames: usize,
    pub channels: Vec<StackAnimChannel>,
}

impl StackAnimationFunction {
    pub fn end_frame(&self) -> usize {
        self.start_frame.saturating_add(self.duration_frames.max(1))
    }

    pub fn contains_frame(&self, frame: usize) -> bool {
        frame >= self.start_frame && frame <= self.end_frame()
    }

    /// Relative local frame: **0** at stack start, up to `duration_frames` at stack end
    /// (independent of global timeline frame numbers).
    pub fn local_frame(&self, global_frame: usize) -> f64 {
        global_frame.saturating_sub(self.start_frame) as f64
    }

    /// Normalized relative time `t` in \[0,1\]: 0 at stack start, 1 at stack end.
    pub fn t_at(&self, frame: usize) -> f64 {
        let dur = self.duration_frames.max(1) as f64;
        (self.local_frame(frame) / dur).clamp(0.0, 1.0)
    }

    fn channel_starts(&self) -> (f64, f64, f64, f64, f64, f64) {
        let mut x = 0.0;
        let mut y = 0.0;
        let mut r = 0.0;
        let mut g = 0.0;
        let mut b = 0.0;
        let mut a = 1.0;
        for ch in &self.channels {
            match ch.track.as_str() {
                "pos_x" => x = ch.start_value,
                "pos_y" => y = ch.start_value,
                "color_r" => r = ch.start_value,
                "color_g" => g = ch.start_value,
                "color_b" => b = ch.start_value,
                "color_a" => a = ch.start_value,
                _ => {}
            }
        }
        // Single-channel stacks: expose start as x as well.
        if self.channels.len() == 1 {
            x = self.channels[0].start_value;
        }
        (x, y, r, g, b, a)
    }

    fn vars_for(&self, track: &str, frame: usize) -> Option<ExprVars> {
        if !self.contains_frame(frame) {
            return None;
        }
        let ch = self.channels.iter().find(|c| c.track == track)?;
        let (x, y, r, g, b, a) = self.channel_starts();
        Some(ExprVars {
            t: self.t_at(frame),
            f: self.local_frame(frame),
            s: ch.start_value,
            x,
            y,
            r,
            g,
            b,
            a,
        })
    }

    pub fn sample_channel(&mut self, track: &str, frame: usize) -> Option<f64> {
        let vars = self.vars_for(track, frame)?;
        let ch = self.channels.iter_mut().find(|c| c.track == track)?;
        let expr = ch.expr.trim().to_string();
        let start_value = ch.start_value;
        if expr.is_empty() {
            ch.last_error = None;
            // Constant start for the whole span (keyframes at start edit this via sync).
            return Some(start_value);
        }
        match eval_expr_vars(&expr, vars) {
            Ok(v) => {
                ch.last_error = None;
                Some(v)
            }
            Err(e) => {
                ch.last_error = Some(e.0);
                Some(start_value)
            }
        }
    }

    /// Read-only sample (does not store errors on channels).
    pub fn sample_channel_ref(&self, track: &str, frame: usize) -> Result<Option<f64>, String> {
        let Some(vars) = self.vars_for(track, frame) else {
            return Ok(None);
        };
        let ch = self
            .channels
            .iter()
            .find(|c| c.track == track)
            .ok_or_else(|| format!("no channel for {track}"))?;
        let expr = ch.expr.trim();
        if expr.is_empty() {
            return Ok(Some(ch.start_value));
        }
        eval_expr_vars(expr, vars)
            .map(Some)
            .map_err(|e| e.0)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NodeAnimation {
    pub pos_x: KeyframeTrack,
    pub pos_y: KeyframeTrack,
    pub rotation: KeyframeTrack,
    pub opacity: KeyframeTrack,
    pub color_r: KeyframeTrack,
    pub color_g: KeyframeTrack,
    pub color_b: KeyframeTrack,
    pub color_a: KeyframeTrack,
    #[serde(default)]
    pub geom_tracks: Vec<KeyframeTrack>,
    #[serde(default)]
    pub base_fill: Option<Fill>,
    /// Formula-driven spans from the Graph Editor (override keyframes while active).
    #[serde(default)]
    pub stack_functions: Vec<StackAnimationFunction>,
}

impl NodeAnimation {
    pub fn get_track_mut(&mut self, label: &str) -> Option<&mut KeyframeTrack> {
        match label {
            "pos_x" => Some(&mut self.pos_x),
            "pos_y" => Some(&mut self.pos_y),
            "rotation" => Some(&mut self.rotation),
            "opacity" => Some(&mut self.opacity),
            "color_r" => Some(&mut self.color_r),
            "color_g" => Some(&mut self.color_g),
            "color_b" => Some(&mut self.color_b),
            "color_a" => Some(&mut self.color_a),
            _ if label.starts_with("geom_") => {
                let idx: usize = label["geom_".len()..].parse().ok()?;
                self.geom_tracks.get_mut(idx)
            }
            _ => None,
        }
    }

    pub fn get_track(&self, label: &str) -> Option<&KeyframeTrack> {
        match label {
            "pos_x" => Some(&self.pos_x),
            "pos_y" => Some(&self.pos_y),
            "rotation" => Some(&self.rotation),
            "opacity" => Some(&self.opacity),
            "color_r" => Some(&self.color_r),
            "color_g" => Some(&self.color_g),
            "color_b" => Some(&self.color_b),
            "color_a" => Some(&self.color_a),
            _ if label.starts_with("geom_") => {
                let idx: usize = label["geom_".len()..].parse().ok()?;
                self.geom_tracks.get(idx)
            }
            _ => None,
        }
    }

    /// Pull stack `start_value` from keyframes at each stack's start frame (editable initial).
    pub fn sync_stack_starts_from_keyframes(&mut self) {
        // Two-phase to avoid borrow conflicts.
        let updates: Vec<(Uuid, String, f64)> = self
            .stack_functions
            .iter()
            .flat_map(|sf| {
                sf.channels.iter().filter_map(|ch| {
                    let track = self.get_track(&ch.track)?;
                    let kf = track.keyframes.iter().find(|k| k.frame == sf.start_frame)?;
                    Some((sf.id, ch.track.clone(), kf.value))
                })
            })
            .collect();
        for (sid, track, val) in updates {
            if let Some(sf) = self.stack_functions.iter_mut().find(|s| s.id == sid) {
                if let Some(ch) = sf.channels.iter_mut().find(|c| c.track == track) {
                    ch.start_value = val;
                }
            }
        }
    }

    /// Ensure each stack channel has a keyframe at start_frame (= start_value).
    pub fn ensure_stack_start_keyframes(&mut self) {
        let needed: Vec<(String, usize, f64)> = self
            .stack_functions
            .iter()
            .flat_map(|sf| {
                sf.channels
                    .iter()
                    .map(|ch| (ch.track.clone(), sf.start_frame, ch.start_value))
            })
            .collect();
        for (track, frame, val) in needed {
            if let Some(tr) = self.get_track_mut(&track) {
                if let Some(kf) = tr.keyframes.iter_mut().find(|k| k.frame == frame) {
                    // Keep user-edited keyframe value; start_value sync handles the reverse.
                    let _ = kf;
                } else {
                    tr.insert(frame, val);
                }
            }
        }
    }

    /// Sample a track: stack functions win inside their span, else keyframes.
    /// After a stack ends, the end keyframe (t=1 value) holds until later keys.
    pub fn sample(&self, label: &str, frame: usize) -> Option<f64> {
        for sf in &self.stack_functions {
            if let Ok(Some(v)) = sf.sample_channel_ref(label, frame) {
                return Some(v);
            }
        }
        self.get_track(label)?.interpolate(frame)
    }

    /// Mutating sample that records formula errors on channels.
    pub fn sample_mut(&mut self, label: &str, frame: usize) -> Option<f64> {
        self.sync_stack_starts_from_keyframes();
        // Keep end anchors in sync so post-stack hold uses expr end, not start.
        self.ensure_stack_end_keyframes();
        for sf in &mut self.stack_functions {
            if let Some(v) = sf.sample_channel(label, frame) {
                return Some(v);
            }
        }
        self.get_track(label)?.interpolate(frame)
    }

    /// Delete keyframes strictly inside (start, end) on the given tracks; keep endpoints.
    pub fn clear_keyframes_in_open_span(&mut self, tracks: &[&str], start: usize, end: usize) {
        for label in tracks {
            if let Some(track) = self.get_track_mut(label) {
                track
                    .keyframes
                    .retain(|kf| kf.frame <= start || kf.frame >= end);
            }
        }
    }

    /// Clear keyframes strictly inside the stack span; keep start (and later end) keyframes.
    pub fn clear_keyframes_under_stack(
        &mut self,
        tracks: &[&str],
        start: usize,
        end: usize,
    ) {
        for label in tracks {
            if let Some(track) = self.get_track_mut(label) {
                track.keyframes.retain(|kf| {
                    // Keep start + end anchors; drop interiors.
                    kf.frame == start || kf.frame == end || kf.frame < start || kf.frame > end
                });
            }
        }
    }

    /// Write/update keyframes at each stack's **end** to the expression value at t=1,
    /// so playback holds the ending point after the span (no snap-back to start).
    pub fn ensure_stack_end_keyframes(&mut self) {
        let mut needed: Vec<(String, usize, f64)> = Vec::new();
        for sf in &self.stack_functions {
            let end = sf.end_frame();
            for ch in &sf.channels {
                let v = sf
                    .sample_channel_ref(&ch.track, end)
                    .ok()
                    .flatten()
                    .unwrap_or(ch.start_value);
                needed.push((ch.track.clone(), end, v));
            }
        }
        for (track, frame, val) in needed {
            if let Some(tr) = self.get_track_mut(&track) {
                tr.insert(frame, val);
            }
        }
    }

    pub fn remove_stack_function(&mut self, id: Uuid) -> bool {
        let before = self.stack_functions.len();
        self.stack_functions.retain(|s| s.id != id);
        self.stack_functions.len() != before
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AnimationTimeline {
    pub nodes: std::collections::HashMap<NodeId, NodeAnimation>,
}
