//! Procedural systems (Phase 15): bulk creation, tracks, and layout.
//!
//! The core performance principle: one Lua call stages a whole batch.
//! ```lua
//! local pts = vblua.batch.grid({ x = 0, y = 0 }, 10, 60, 60, 100)
//! local specs = {}
//! for i, p in ipairs(pts) do
//!   specs[i] = { kind = "Blur", x = p.x, y = p.y, name = "B" .. i }
//! end
//! local ids = vblua.batch.create_nodes(specs)  -- ONE call, 100 nodes
//! vblua.batch.keyframes(id, "rotation", {{ 0, 0 }, { 60, 360 }})
//! ```
//!
//! Everything routes through the standard queues (same validation, same
//! undo). Budgets: 512 specs / 2048 keyframe points per call, always capped
//! by the policy `max_commands`.

use super::animation::AnimCommand;
use super::graph::{GraphCommand, ParamValue};

pub const MAX_BATCH_NODES: usize = 512;
pub const MAX_BATCH_KEYS: usize = 2048;

/// One bulk node spec (decoded from Lua, validated by `check_node_spec`).
pub struct NodeSpec {
    pub kind: crate::document::GraphNodeKind,
    pub kind_name: String,
    pub x: f32,
    pub y: f32,
    pub name: Option<String>,
    pub params: Vec<(String, ParamValue)>,
}

/// Validate one decoded spec (kind allowlist + param keys/types).
pub fn check_node_spec(spec: NodeSpec) -> Result<NodeSpec, String> {
    for (k, v) in &spec.params {
        let mut probe = spec.kind.clone();
        super::graph::set_node_param(&mut probe, k, v.clone())
            .map_err(|e| format!("param '{k}': {e}"))?;
    }
    Ok(spec)
}

/// Grid positions: `count` points, `cols` per row.
pub fn grid(
    origin: [f64; 2],
    cols: usize,
    dx: f64,
    dy: f64,
    count: usize,
) -> Vec<[f64; 2]> {
    let cols = cols.max(1);
    (0..count)
        .map(|i| {
            [
                origin[0] + (i % cols) as f64 * dx,
                origin[1] + (i / cols) as f64 * dy,
            ]
        })
        .collect()
}

/// Circle positions: `count` points around `center` (optional phase radians).
pub fn circle(center: [f64; 2], radius: f64, count: usize, phase: f64) -> Vec<[f64; 2]> {
    if count == 0 {
        return Vec::new();
    }
    let r = radius.max(0.0);
    (0..count)
        .map(|i| {
            let a = phase + 2.0 * std::f64::consts::PI * i as f64 / count as f64;
            [center[0] + r * a.cos(), center[1] + r * a.sin()]
        })
        .collect()
}

/// Queue bulk creates. Returns `(id, kind, label)` per spec so the caller
/// can update optimistic snapshots (kinds map + node rows).
#[allow(clippy::too_many_arguments)]
pub fn queue_creates(
    queue: &mut Vec<GraphCommand>,
    layer_id: uuid::Uuid,
    specs: Vec<NodeSpec>,
    origin: [f32; 2],
) -> Vec<(uuid::Uuid, crate::document::GraphNodeKind, String, f32, f32)> {
    let mut out = Vec::with_capacity(specs.len());
    for spec in specs {
        let id = uuid::Uuid::new_v4();
        let label = spec.name.clone().unwrap_or_else(|| {
            super::graph::kind_label(&spec.kind).to_string()
        });
        let (nx, ny) = (
            (origin[0] + spec.x).clamp(-4000.0, 4000.0),
            (origin[1] + spec.y).clamp(-4000.0, 4000.0),
        );
        queue.push(GraphCommand::CreateNode {
            id,
            layer_id,
            kind: spec.kind.clone(),
            x: nx,
            y: ny,
            name: spec.name,
        });
        for (k, v) in &spec.params {
            queue.push(GraphCommand::SetNodeParam {
                node_id: id,
                key: k.clone(),
                value: v.clone(),
            });
        }
        out.push((id, spec.kind, label, nx, ny));
    }
    out
}

/// Queue a keyframe sequence on one track. Returns the staged count.
pub fn queue_keyframes(
    queue: &mut Vec<AnimCommand>,
    target: uuid::Uuid,
    track: String,
    points: Vec<(usize, f64, crate::document::InterpolationMode)>,
) {
    for (frame, value, interp) in points {
        queue.push(AnimCommand::SetKeyframe {
            target,
            track: track.clone(),
            frame,
            value,
            interp,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_math() {
        let g = grid([0.0, 0.0], 3, 10.0, 20.0, 4);
        assert_eq!(g, vec![[0.0, 0.0], [10.0, 0.0], [20.0, 0.0], [0.0, 20.0]]);
        let c = circle([0.0, 0.0], 10.0, 4, 0.0);
        assert!((c[0][0] - 10.0).abs() < 1e-9 && c[0][1].abs() < 1e-9);
        assert!((c[2][0] + 10.0).abs() < 1e-9);
        assert!(circle([0.0, 0.0], 5.0, 0, 0.0).is_empty());
    }

    #[test]
    fn spec_validation_rejects_bad_params() {
        let kind = crate::document::GraphNodeKind::Value { value: 0.0 };
        assert!(
            check_node_spec(NodeSpec {
                kind,
                kind_name: "Value".into(),
                x: 0.0,
                y: 0.0,
                name: None,
                params: vec![("value".into(), ParamValue::Num(3.0))],
            })
            .is_ok()
        );
        let kind = crate::document::GraphNodeKind::LinearBlur;
        assert!(
            check_node_spec(NodeSpec {
                kind,
                kind_name: "Blur".into(),
                x: 0.0,
                y: 0.0,
                name: None,
                params: vec![("radius".into(), ParamValue::Num(3.0))],
            })
            .is_err()
        );
    }
}
