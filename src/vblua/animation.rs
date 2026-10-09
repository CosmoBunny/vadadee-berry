//! Animation API (Phase 7): keyframes over canvas-node / layer tracks.
//!
//! Lua shape:
//! ```lua
//! vblua.animation.set_keyframe(node_id, "rotation", 0, 0)
//! vblua.animation.set_keyframe(node_id, "rotation", 30, 360, "bezier")
//! vblua.animation.sample(node_id, "rotation", 15) -- 180.0
//! vblua.animation.keyframes(node_id, "rotation")   -- [{frame, value, interp}]
//! vblua.animation.remove_keyframe(node_id, "rotation", 30)
//! ```
//!
//! Tracks mirror the host (`pos_x/y, rotation, opacity, color_r/g/b/a,
//! stroke_width, stroke_r/g/b/a, geom_N, param:*`). Interpolation is
//! `linear` (default) or `bezier` — the host has no step/smooth/cubic modes,
//! so those fail loudly instead of silently degrading. Stack-function spans
//! are out of scope (sampled from keyframes only).

use std::collections::{HashMap, HashSet};

use crate::document::{
    AnimationTimeline, InterpolationMode, KeyframeTrack, ProjectFile,
};

/// Fixed scriptable track labels (plus `geom_N` and `param:*` patterns).
pub const TRACK_LABELS: &[&str] = &[
    "pos_x",
    "pos_y",
    "rotation",
    "opacity",
    "color_r",
    "color_g",
    "color_b",
    "color_a",
    "stroke_width",
    "stroke_r",
    "stroke_g",
    "stroke_b",
    "stroke_a",
];

/// Validate a track label (fixed set + `geom_N` + `param:*`).
pub fn valid_track(label: &str) -> bool {
    if TRACK_LABELS.contains(&label) {
        return true;
    }
    if let Some(rest) = label.strip_prefix("geom_")
        && rest.parse::<usize>().is_ok()
    {
        return true;
    }
    if label.starts_with("param:") && label.len() > "param:".len() {
        return true;
    }
    false
}

/// Parse interpolation (`linear` default, `bezier`). Everything else errors.
pub fn parse_interp(name: Option<&str>) -> Result<InterpolationMode, String> {
    match name.map(|s| s.trim().to_lowercase()).as_deref() {
        None | Some("") | Some("linear") => Ok(InterpolationMode::Linear),
        Some("bezier") | Some("smooth") => {
            // NOTE: "smooth" is accepted as an alias for bezier (closest host
            // mode), documented as such. True smooth/cubic-spline is future work.
            Ok(InterpolationMode::Bezier)
        }
        Some(other) => Err(format!(
            "unknown interpolation '{other}' — use linear or bezier (step/cubic/custom are not host modes)"
        )),
    }
}

/// One keyframe row (snapshot). `hr` mirrors the host bezier handle so
/// snapshot sampling matches `KeyframeTrack::interpolate` exactly.
#[derive(Debug, Clone)]
pub struct KfSnapshot {
    pub frame: usize,
    pub value: f64,
    pub interp: &'static str,
    pub hr: (f64, f64),
}

/// One animated target (snapshot).
#[derive(Debug, Clone, Default)]
pub struct AnimNodeSnapshot {
    pub name: String,
    pub tracks: HashMap<String, Vec<KfSnapshot>>,
}

/// Timeline snapshot + the set of animatable ids (canvas nodes + layers).
#[derive(Debug, Clone, Default)]
pub struct AnimSnapshot {
    pub nodes: HashMap<String, AnimNodeSnapshot>,
    /// Every id scripts may target (node ids + layer ids as strings).
    pub targets: HashSet<String>,
    /// `content_max_animation_frame(project, 60)` at capture time.
    pub max_frame_60: usize,
}

impl AnimSnapshot {
    pub fn capture(project: &ProjectFile) -> Self {
        let mut targets = HashSet::new();
        for id in project.nodes.map.keys() {
            targets.insert(id.to_string());
        }
        for l in &project.document.layers {
            targets.insert(l.id.to_string());
        }
        let mut nodes = HashMap::new();
        for (id, anim) in &project.anim_timeline.nodes {
            let name = project
                .nodes
                .map
                .get(id)
                .map(|n| n.name.clone())
                .unwrap_or_default();
            let mut tracks = HashMap::new();
            collect_tracks(anim, &mut tracks);
            nodes.insert(
                id.to_string(),
                AnimNodeSnapshot { name, tracks },
            );
        }
        Self {
            nodes,
            targets,
            max_frame_60: crate::document::content_max_animation_frame(project, 60),
        }
    }
}

fn collect_tracks(
    anim: &crate::document::NodeAnimation,
    out: &mut HashMap<String, Vec<KfSnapshot>>,
) {
    let mut one = |label: &str, track: &KeyframeTrack| {
        if !track.keyframes.is_empty() {
            out.insert(
                label.to_string(),
                track
                    .keyframes
                    .iter()
                    .map(|k| KfSnapshot {
                        frame: k.frame,
                        value: k.value,
                        interp: match k.interpolation {
                            InterpolationMode::Linear => "linear",
                            InterpolationMode::Bezier => "bezier",
                        },
                        hr: k.handle_right,
                    })
                    .collect(),
            );
        }
    };
    one("pos_x", &anim.pos_x);
    one("pos_y", &anim.pos_y);
    one("rotation", &anim.rotation);
    one("opacity", &anim.opacity);
    one("color_r", &anim.color_r);
    one("color_g", &anim.color_g);
    one("color_b", &anim.color_b);
    one("color_a", &anim.color_a);
    one("stroke_width", &anim.stroke_width);
    one("stroke_r", &anim.stroke_r);
    one("stroke_g", &anim.stroke_g);
    one("stroke_b", &anim.stroke_b);
    one("stroke_a", &anim.stroke_a);
    for (i, t) in anim.geom_tracks.iter().enumerate() {
        one(&format!("geom_{i}"), t);
    }
    for (label, t) in &anim.param_tracks {
        one(label, t);
    }
}

/// Timeline signature: total keyframe count (console undo detection).
pub fn timeline_signature(timeline: &AnimationTimeline) -> usize {
    timeline
        .nodes
        .values()
        .map(|a| {
            let mut n = a.pos_x.keyframes.len()
                + a.pos_y.keyframes.len()
                + a.rotation.keyframes.len()
                + a.opacity.keyframes.len()
                + a.color_r.keyframes.len()
                + a.color_g.keyframes.len()
                + a.color_b.keyframes.len()
                + a.color_a.keyframes.len()
                + a.stroke_width.keyframes.len()
                + a.stroke_r.keyframes.len()
                + a.stroke_g.keyframes.len()
                + a.stroke_b.keyframes.len()
                + a.stroke_a.keyframes.len();
            n += a.geom_tracks.iter().map(|t| t.keyframes.len()).sum::<usize>();
            n += a.param_tracks.values().map(|t| t.keyframes.len()).sum::<usize>();
            n
        })
        .sum()
}

/// One staged timeline mutation.
#[derive(Debug, Clone)]
pub enum AnimCommand {
    SetKeyframe {
        target: uuid::Uuid,
        track: String,
        frame: usize,
        value: f64,
        interp: InterpolationMode,
    },
    RemoveKeyframe {
        target: uuid::Uuid,
        track: String,
        frame: usize,
    },
}

impl AnimCommand {
    /// Apply one command; `true` when the timeline changed.
    pub fn apply_to(&self, timeline: &mut AnimationTimeline) -> bool {
        match self {
            AnimCommand::SetKeyframe {
                target,
                track,
                frame,
                value,
                interp,
            } => {
                let node_id = *target;
                let anim = timeline.nodes.entry(node_id).or_default();
                let Some(t) = anim.get_track_mut(track) else {
                    return false;
                };
                t.insert(*frame, *value);
                if let Some(kf) = t.keyframes.iter_mut().find(|k| k.frame == *frame) {
                    kf.interpolation = *interp;
                }
                true
            }
            AnimCommand::RemoveKeyframe {
                target,
                track,
                frame,
            } => {
                let Some(anim) = timeline.nodes.get_mut(target) else {
                    return false;
                };
                let Some(t) = anim.get_track_mut(track) else {
                    return false;
                };
                let before = t.keyframes.len();
                t.keyframes.retain(|k| k.frame != *frame);
                before != t.keyframes.len()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_and_interp_validation() {
        assert!(valid_track("rotation"));
        assert!(valid_track("geom_12"));
        assert!(valid_track("param:abc:0"));
        assert!(!valid_track("radius"));
        assert!(!valid_track("geom_"));
        assert!(parse_interp(None).is_ok());
        assert!(parse_interp(Some("bezier")).is_ok());
        assert!(parse_interp(Some("step")).is_err());
        assert!(parse_interp(Some("custom")).is_err());
    }

    #[test]
    fn set_remove_round_trip() {
        let mut tl = AnimationTimeline::default();
        let id = uuid::Uuid::new_v4();
        assert!(AnimCommand::SetKeyframe {
            target: id,
            track: "rotation".into(),
            frame: 30,
            value: 360.0,
            interp: InterpolationMode::Bezier,
        }
        .apply_to(&mut tl));
        let anim = tl.nodes.get(&id).unwrap();
        assert_eq!(anim.rotation.keyframes.len(), 1);
        assert_eq!(anim.rotation.keyframes[0].interpolation, InterpolationMode::Bezier);
        // Same frame replaces.
        assert!(AnimCommand::SetKeyframe {
            target: id,
            track: "rotation".into(),
            frame: 30,
            value: 180.0,
            interp: InterpolationMode::Linear,
        }
        .apply_to(&mut tl));
        assert_eq!(tl.nodes.get(&id).unwrap().rotation.keyframes.len(), 1);
        assert!(AnimCommand::RemoveKeyframe {
            target: id,
            track: "rotation".into(),
            frame: 30,
        }
        .apply_to(&mut tl));
        assert!(tl.nodes.get(&id).unwrap().rotation.keyframes.is_empty());
        // Unknown track is a no-op.
        assert!(!AnimCommand::SetKeyframe {
            target: id,
            track: "radius".into(),
            frame: 0,
            value: 1.0,
            interp: InterpolationMode::Linear,
        }
        .apply_to(&mut tl));
    }

    #[test]
    fn snapshot_sample_matches_host_bezier() {
        use crate::document::{InterpolationMode, KeyframeTrack};
        let mut track = KeyframeTrack::default();
        track.insert(0, 0.0);
        track.insert(30, 360.0);
        track.keyframes[0].interpolation = InterpolationMode::Bezier;
        track.keyframes[0].handle_right = (10.0, 40.0);
        let snap = KfSnapshot { frame: 0, value: 0.0, interp: "bezier", hr: (10.0, 40.0) };
        let snap2 = KfSnapshot { frame: 30, value: 360.0, interp: "linear", hr: (5.0, 0.0) };
        for frame in [0usize, 5, 15, 29, 30] {
            let host = track.interpolate(frame).unwrap();
            let ours = super::super::api::interpolate_snapshot(&[snap.clone(), snap2.clone()], frame).unwrap();
            assert!((host - ours).abs() < 1e-9, "frame {frame}: host {host} vs {ours}");
        }
    }
}
