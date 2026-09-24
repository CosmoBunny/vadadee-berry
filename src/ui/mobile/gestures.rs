//! Touch/stylus input as editor intents.
//!
//! Recognition is egui's job (`multi_touch()` for pinch/pan/rotate;
//! pointer emulation for single-finger tap/drag) — this module must NOT
//! reimplement gesture detection. It converts recognized gestures into
//! [`EditorIntent`]s, the vocabulary Phase 3 routes into shared commands.
//!
//! Viewport motion itself stays where it already works: `App::canvas_wheel_zoom`
//! applies egui's multi-touch deltas with correct canvas-origin anchoring on
//! every frame, shell-independent. One-finger editing flows through the
//! existing canvas tools via egui pointer emulation.
//!
//! Known Phase-3 gap: while two fingers pinch, finger one's drag still
//! reaches tool input (object may move under the gesture). Fixing that means
//! routing tool input through intents with a pinch-suppression rule — not a
//! second recognizer.

/// One user intention, independent of how it was produced (fingers, stylus,
/// wheel, keys). Positions are screen points; the canvas maps them to
/// document space exactly like pointer input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EditorIntent {
    Tap {
        position: egui::Pos2,
    },
    DoubleTap {
        position: egui::Pos2,
    },
    Drag {
        start: egui::Pos2,
        current: egui::Pos2,
    },
    PinchZoom {
        center: egui::Pos2,
        /// Multiplicative zoom factor since gesture start (>1 zooms in).
        scale: f32,
    },
    Pan {
        delta: egui::Vec2,
    },
    Rotate {
        center: egui::Pos2,
        /// Radians since gesture start.
        angle: f32,
    },
    LongPress {
        position: egui::Pos2,
    },
}

/// Convert one egui multi-touch snapshot into intents. Pure function over
/// the snapshot — unit-testable without a `Context`. Thresholds mirror the
/// desktop zoom path (`0.5px` span noise, `1e-4` zoom epsilon).
pub fn intents_from_multitouch(mt: &egui::MultiTouchInfo) -> Vec<EditorIntent> {
    let mut out = Vec::new();
    if (mt.zoom_delta - 1.0).abs() > 1e-4 {
        out.push(EditorIntent::PinchZoom {
            center: mt.center_pos,
            scale: mt.zoom_delta,
        });
    }
    if mt.translation_delta != egui::Vec2::ZERO {
        out.push(EditorIntent::Pan {
            delta: mt.translation_delta,
        });
    }
    if mt.rotation_delta.abs() > 1e-4 {
        out.push(EditorIntent::Rotate {
            center: mt.center_pos,
            angle: mt.rotation_delta,
        });
    }
    out
}

/// Collect this frame's gesture intents from the egui input state.
/// Single-finger tap/drag/long-press are intentionally absent: egui delivers
/// those as pointer events straight to the canvas tools, and re-emitting
/// them here would double-handle every touch.
pub fn collect_frame_intents(ctx: &egui::Context) -> Vec<EditorIntent> {
    ctx.input(|i| {
        i.multi_touch()
            .map(|mt| intents_from_multitouch(&mt))
            .unwrap_or_default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn multitouch(zoom: f32, pan: egui::Vec2, rot: f32) -> egui::MultiTouchInfo {
        egui::MultiTouchInfo {
            start_time: 0.0,
            start_pos: egui::pos2(0.0, 0.0),
            center_pos: egui::pos2(100.0, 100.0),
            num_touches: 2,
            zoom_delta: zoom,
            zoom_delta_2d: egui::vec2(zoom, zoom),
            rotation_delta: rot,
            translation_delta: pan,
            force: 0.5,
        }
    }

    #[test]
    fn idle_gesture_yields_no_intents() {
        assert!(intents_from_multitouch(&multitouch(1.0, egui::Vec2::ZERO, 0.0)).is_empty());
    }

    #[test]
    fn pinch_spread_reports_zoom_out_scale() {
        let intents = intents_from_multitouch(&multitouch(1.2, egui::Vec2::ZERO, 0.0));
        assert_eq!(
            intents,
            vec![EditorIntent::PinchZoom {
                center: egui::pos2(100.0, 100.0),
                scale: 1.2,
            }]
        );
    }

    #[test]
    fn pan_and_rotate_compose() {
        let intents = intents_from_multitouch(&multitouch(1.0, egui::vec2(5.0, -3.0), 0.1));
        assert_eq!(intents.len(), 2);
        assert!(intents.contains(&EditorIntent::Pan {
            delta: egui::vec2(5.0, -3.0)
        }));
        assert!(intents.contains(&EditorIntent::Rotate {
            center: egui::pos2(100.0, 100.0),
            angle: 0.1
        }));
    }

    #[test]
    fn full_gesture_reports_all_three() {
        let intents = intents_from_multitouch(&multitouch(0.8, egui::vec2(1.0, 1.0), -0.2));
        assert_eq!(intents.len(), 3);
    }
}
