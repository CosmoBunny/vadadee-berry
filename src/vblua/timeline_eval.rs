//! Timeline evaluation (R3a): run registered scripts against the playhead.
//!
//! Rules (see `docs/VBLUA_REVIEW_TODO.md`):
//! - Static scripts (never touch time APIs) run once, then are skipped until
//!   their source changes.
//! - Temporal scripts run when the playhead is inside their last declared
//!   range (seconds convert at the *current* fps; frames are direct).
//!   Temporal scripts with no declared range run every frame.
//! - One fresh permissive runtime per frame: no cross-talk between frames,
//!   scrub-safe and deterministic. Commands apply with NO history push —
//!   playback effects, like animation sampling. Scripts must be functions
//!   of the playhead; repeating a frame repeats its effects exactly.
//! - Trust: registry scripts auto-run on playback (experimental channel).
//!   The per-script enable toggle is the control — open trusted projects.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use uuid::Uuid;

use super::runtime::VbRuntime;
use super::sandbox::SandboxPolicy;
use crate::document::ProjectFile;

/// Registry scripts evaluated per frame change (creation caps at this).
pub const MAX_SCRIPTS_PER_FRAME: usize = 32;

/// Session-side eval state per script (NOT persisted — ranges re-learn).
#[derive(Debug, Clone, Default)]
pub struct ScriptEvalState {
    pub source_hash: u64,
    pub seen: bool,
    pub temporal: bool,
    /// Last declared domain: (start, end, is_frames).
    pub range: Option<(f64, f64, bool)>,
}

/// What one frame evaluation did (errors never abort other scripts).
#[derive(Debug, Default)]
pub struct FrameReport {
    pub ran: Vec<String>,
    pub errors: Vec<(String, String)>,
}

fn hash_source(source: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut h);
    h.finish()
}

/// Playhead-in-range check (seconds convert at current fps).
pub fn range_contains(range: (f64, f64, bool), frame: usize, fps: u32) -> bool {
    let f = frame as f64;
    let (s, e) = if range.2 {
        (range.0, range.1)
    } else {
        (
            range.0 * fps.max(1) as f64,
            range.1 * fps.max(1) as f64,
        )
    };
    f >= s && f <= e
}

/// Latest frame a session's recorded script ranges reach (fps-aware).
/// Pure helper behind the playable-span fold: enabled temporal scripts only.
pub fn recorded_span_end_frame(
    scripts: &[(Uuid, bool)],
    states: &HashMap<Uuid, ScriptEvalState>,
    fps: u32,
) -> usize {
    let mut end = 0usize;
    for (id, enabled) in scripts {
        if !enabled {
            continue;
        }
        let Some(st) = states.get(id) else {
            continue;
        };
        if !st.temporal {
            continue;
        }
        if let Some((_, b, is_frames)) = st.range {
            let f = if is_frames {
                b
            } else {
                b * fps.max(1) as f64
            };
            end = end.max(f.ceil().max(0.0) as usize);
        }
    }
    end
}

/// Export Duration (seconds, 0 = Auto) expressed in frames at `fps`.
/// Pure helper behind the span fold (span takes the max, never a cap).
pub fn export_duration_end_frame(duration_secs: f32, fps: u32) -> usize {
    if duration_secs <= 1e-6 {
        return 0;
    }
    (duration_secs.max(0.0) * fps.max(1) as f32).ceil().max(0.0) as usize
}

/// Evaluate registered scripts against `frame`. Applies staged commands
/// directly — no undo entries (see module docs).
pub fn evaluate_frame(
    project: &mut ProjectFile,
    frame: usize,
    fps: u32,
    states: &mut HashMap<Uuid, ScriptEvalState>,
) -> FrameReport {
    let mut report = FrameReport::default();
    let scripts: Vec<(Uuid, String, String, bool)> = project
        .document
        .timeline_scripts
        .iter()
        .take(MAX_SCRIPTS_PER_FRAME)
        .map(|s| (s.id, s.name.clone(), s.source.clone(), s.enabled))
        .collect();
    if scripts.is_empty() {
        return report;
    }
    let fps = fps.max(1);
    let Ok(mut rt) = VbRuntime::new(SandboxPolicy::permissive_for_tests()) else {
        return report;
    };
    for (id, name, source, enabled) in scripts {
        if !enabled {
            continue;
        }
        let h = hash_source(&source);
        let st = states.entry(id).or_insert(ScriptEvalState {
            source_hash: h,
            ..Default::default()
        });
        if st.source_hash != h {
            *st = ScriptEvalState {
                source_hash: h,
                ..Default::default()
            };
        }
        // Static scripts ran once — skip until edited.
        if st.seen && !st.temporal {
            continue;
        }
        // Temporal scripts gate on their declared range (undeclared = always).
        if st.seen && st.temporal {
            if let Some(r) = st.range {
                if !range_contains(r, frame, fps) {
                    continue;
                }
            }
        }
        rt.set_playback(frame, fps);
        let res = rt.execute_with_project(&name, &source, Some(project));
        let (touched, declared) = rt.take_timeline_marks();
        st.seen = true;
        if touched {
            st.temporal = true;
        }
        if declared.is_some() {
            st.range = declared;
        }
        match res {
            Ok(_) => report.ran.push(name),
            Err(e) => report.errors.push((name, e.render())),
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{NodeStore, TimelineScript};

    fn project_with(source: &str) -> ProjectFile {
        let mut doc = crate::document::Document::new_empty_project().document;
        doc.timeline_scripts
            .push(TimelineScript::new("T", source));
        ProjectFile::new(doc, NodeStore::default())
    }

    #[test]
    fn static_script_runs_once() {
        let mut project = project_with(r#"STATIC = (STATIC or 0) + 1"#);
        // Fresh runtime per frame would reset Lua globals — but the R3
        // contract is one runtime per frame, so repeated eval must not
        // depend on Lua globals. Static gating uses session state instead:
        // first frame runs, second frame skips (no state change to observe,
        // so assert via the report).
        let mut states = HashMap::new();
        let r1 = evaluate_frame(&mut project, 0, 30, &mut states);
        assert_eq!(r1.ran.len(), 1);
        let r2 = evaluate_frame(&mut project, 10, 30, &mut states);
        assert!(r2.ran.is_empty(), "static must run once");
    }

    #[test]
    fn temporal_range_gates_frames_and_seconds() {
        let mut project = project_with(
            r#"
            vblua.timeline.range(1, 3, "seconds")
            SEEN = vblua.timeline.time()
            "#,
        );
        let mut states = HashMap::new();
        // 1..3s @30fps = frames 30..90.
        let r0 = evaluate_frame(&mut project, 0, 30, &mut states);
        assert_eq!(r0.ran.len(), 1, "first run learns the range");
        assert!(evaluate_frame(&mut project, 10, 30, &mut states)
            .ran
            .is_empty());
        assert_eq!(
            evaluate_frame(&mut project, 60, 30, &mut states).ran.len(),
            1
        );
        // Boundaries inclusive.
        assert_eq!(
            evaluate_frame(&mut project, 90, 30, &mut states).ran.len(),
            1
        );
        assert!(evaluate_frame(&mut project, 91, 30, &mut states)
            .ran
            .is_empty());
    }

    #[test]
    fn frame_unit_range_is_fps_independent() {
        let mut project = project_with(r#"vblua.timeline.range(0, 120, "frames")"#);
        let mut states = HashMap::new();
        evaluate_frame(&mut project, 0, 30, &mut states); // learn
        assert_eq!(
            evaluate_frame(&mut project, 120, 60, &mut states).ran.len(),
            1,
            "frame ranges ignore fps"
        );
        assert!(evaluate_frame(&mut project, 121, 60, &mut states)
            .ran
            .is_empty());
    }

    #[test]
    fn range_validation_rejects_bad_domains() {
        for bad in [
            r#"vblua.timeline.range(5, 1, "seconds")"#,
            r#"vblua.timeline.range(-1, 5, "seconds")"#,
            r#"vblua.timeline.range(0, 5, "parsecs")"#,
        ] {
            let mut project = project_with(bad);
            let mut states = HashMap::new();
            let r = evaluate_frame(&mut project, 0, 30, &mut states);
            assert_eq!(r.errors.len(), 1, "bad range must error: {bad}");
        }
    }

    #[test]
    fn export_duration_converts_to_frames() {
        assert_eq!(export_duration_end_frame(0.0, 60), 0); // Auto: no fold
        assert_eq!(export_duration_end_frame(5.0, 60), 300);
        assert_eq!(export_duration_end_frame(5.0, 30), 150);
    }

    #[test]
    fn recorded_span_covers_enabled_temporal() {
        let id = Uuid::new_v4();
        let scripts = vec![(id, true), (Uuid::new_v4(), false)];
        let mut states = HashMap::new();
        assert_eq!(recorded_span_end_frame(&scripts, &states, 60), 0);
        states.insert(
            id,
            ScriptEvalState {
                source_hash: 1,
                seen: true,
                temporal: true,
                range: Some((1.0, 15.0, false)),
            },
        );
        // 15s @60fps = 900 frames; disabled script contributes nothing.
        assert_eq!(recorded_span_end_frame(&scripts, &states, 60), 900);
        assert_eq!(recorded_span_end_frame(&scripts, &states, 30), 450);
    }

    #[test]
    fn range_contains_math() {
        assert!(range_contains((30.0, 90.0, true), 30, 30));
        assert!(range_contains((30.0, 90.0, true), 90, 30));
        assert!(!range_contains((30.0, 90.0, true), 91, 30));
        assert!(range_contains((1.0, 3.0, false), 60, 30));
        assert!(range_contains((1.0, 3.0, false), 150, 60));
        assert!(!range_contains((1.0, 3.0, false), 150, 30));
    }
}
