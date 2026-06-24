use serde::{Deserialize, Serialize};
use crate::document::{NodeId, BezierHandleMode, Fill};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnimationTimeline {
    pub nodes: std::collections::HashMap<NodeId, NodeAnimation>,
}
