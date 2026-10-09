//! Kinematics API (Phase 8): FK/IK and motion helpers in canvas 2D space.
//!
//! Lua shape:
//! ```lua
//! local arm = vblua.kinematics.chain({ 100, 80, 60 })
//! local angles = arm.solve({ x = 150, y = 40 }) -- joint angles (radians)
//! local joints = arm.fk(angles)                  -- joint positions
//! ```
//!
//! Deliberately **pure functions**: Rust does the numerics, Lua describes the
//! system. Solvers return angles/positions; writing poses stays with the
//! existing `animation.set_keyframe` batch path (one undo entry), so no new
//! mutation channel is introduced. No document access, no capability needed.

use std::f64::consts::PI;

/// Maximum segments per chain (DoS guard for Lua-driven loops).
pub const MAX_SEGMENTS: usize = 16;
/// Maximum CCD iterations per solve.
pub const MAX_ITERS: usize = 256;

fn norm_angle(mut a: f64) -> f64 {
    while a > PI {
        a -= 2.0 * PI;
    }
    while a <= -PI {
        a += 2.0 * PI;
    }
    a
}

/// Validate segment lengths: 1..=16 finite positives.
pub fn check_lengths(lengths: &[f64]) -> Result<(), String> {
    if lengths.is_empty() || lengths.len() > MAX_SEGMENTS {
        return Err(format!("chain needs 1..={MAX_SEGMENTS} segments"));
    }
    if lengths.iter().any(|l| !l.is_finite() || *l <= 0.0) {
        return Err("segment lengths must be finite positives".to_string());
    }
    Ok(())
}

/// Forward kinematics: joint positions starting at `base` (base itself
/// included as element 0). Angles are absolute (canvas rotation, radians).
pub fn fk(lengths: &[f64], angles: &[f64], base: [f64; 2]) -> Vec<[f64; 2]> {
    let mut pts = Vec::with_capacity(lengths.len() + 1);
    let (mut x, mut y) = (base[0], base[1]);
    pts.push([x, y]);
    for (i, l) in lengths.iter().enumerate() {
        let a = angles.get(i).copied().unwrap_or(0.0);
        x += l * a.cos();
        y += l * a.sin();
        pts.push([x, y]);
    }
    pts
}

/// Analytic two-bone IK. Returns absolute joint angles `(shoulder, elbow)`.
/// Unreachable targets clamp along the base→target ray (always succeeds).
pub fn ik_two_bone(l1: f64, l2: f64, base: [f64; 2], target: [f64; 2]) -> (f64, f64) {
    let dx = target[0] - base[0];
    let dy = target[1] - base[1];
    let dist = dx.hypot(dy).max(1e-9);
    let clamped = dist.min(l1 + l2 - 1e-6).max((l1 - l2).abs() + 1e-6);
    let base_ang = dy.atan2(dx);
    // Elbow interior angle via law of cosines.
    let mut cos_elbow = (l1 * l1 + l2 * l2 - clamped * clamped) / (2.0 * l1 * l2);
    cos_elbow = cos_elbow.clamp(-1.0, 1.0);
    let elbow_rel = PI - cos_elbow.acos();
    // Shoulder offset.
    let mut cos_a = (l1 * l1 + clamped * clamped - l2 * l2) / (2.0 * l1 * clamped);
    cos_a = cos_a.clamp(-1.0, 1.0);
    let shoulder = norm_angle(base_ang - cos_a.acos());
    let elbow = norm_angle(shoulder + elbow_rel);
    (shoulder, elbow)
}

/// CCD IK over `lengths` from `initial` toward `target`. Returns absolute
/// angles. Iterates at most `iters` (clamped to `MAX_ITERS`).
pub fn ik_ccd(
    lengths: &[f64],
    base: [f64; 2],
    target: [f64; 2],
    initial: &[f64],
    iters: usize,
) -> Vec<f64> {
    let mut angles: Vec<f64> = (0..lengths.len())
        .map(|i| initial.get(i).copied().unwrap_or(0.0))
        .collect();
    let iters = iters.min(MAX_ITERS);
    for _ in 0..iters {
        let pts = fk(lengths, &angles, base);
        let end = pts[pts.len() - 1];
        if (end[0] - target[0]).hypot(end[1] - target[1]) < 0.5 {
            break;
        }
        for i in (0..lengths.len()).rev() {
            let pts = fk(lengths, &angles, base);
            let joint = pts[i];
            let end = pts[pts.len() - 1];
            let to_end = (end[1] - joint[1]).atan2(end[0] - joint[0]);
            let to_goal = (target[1] - joint[1]).atan2(target[0] - joint[0]);
            // Absolute-angle parameterization: rotate the whole downstream
            // chain rigidly about joint i.
            let delta = norm_angle(to_goal - to_end);
            for a in angles.iter_mut().skip(i) {
                *a = norm_angle(*a + delta);
            }
        }
    }
    angles
}

/// Exponential damping: one step toward `target` (framerate-independent).
pub fn damp(current: f64, target: f64, lambda: f64, dt: f64) -> f64 {
    if !current.is_finite() || !target.is_finite() {
        return current;
    }
    let l = lambda.clamp(0.0, 50.0);
    let t = dt.clamp(0.0, 1.0);
    current + (target - current) * (1.0 - (-l * t).exp())
}

/// One semi-implicit Euler spring step. Returns `(pos, vel)`.
pub fn spring_step(
    pos: f64,
    vel: f64,
    target: f64,
    stiffness: f64,
    damping: f64,
    dt: f64,
) -> (f64, f64) {
    if ![pos, vel, target].iter().all(|v| v.is_finite()) {
        return (pos, vel);
    }
    let k = stiffness.clamp(0.0, 1000.0);
    let c = damping.clamp(0.0, 200.0);
    let t = dt.clamp(0.0, 0.05);
    let acc = (target - pos) * k - vel * c;
    let vel = vel + acc * t;
    (pos + vel * t, vel)
}

/// Angle (radians) pointing from `from` to `to`.
pub fn look_at(from: [f64; 2], to: [f64; 2]) -> f64 {
    (to[1] - from[1]).atan2(to[0] - from[0])
}

/// Point on a circle: center + radius at `angle` radians.
pub fn orbit(center: [f64; 2], radius: f64, angle: f64) -> [f64; 2] {
    let r = radius.max(0.0);
    [center[0] + r * angle.cos(), center[1] + r * angle.sin()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fk_straight_and_bent() {
        let pts = fk(&[100.0, 50.0], &[0.0, 0.0], [0.0, 0.0]);
        assert_eq!(pts.len(), 3);
        assert!((pts[2][0] - 150.0).abs() < 1e-9 && pts[2][1].abs() < 1e-9);
        let pts = fk(&[100.0], &[PI / 2.0], [10.0, 20.0]);
        assert!((pts[1][0] - 10.0).abs() < 1e-9 && (pts[1][1] - 120.0).abs() < 1e-6);
    }

    #[test]
    fn two_bone_hits_reachable_target() {
        let (a1, a2) = ik_two_bone(100.0, 80.0, [0.0, 0.0], [150.0, 40.0]);
        let pts = fk(&[100.0, 80.0], &[a1, a2], [0.0, 0.0]);
        let end = pts[2];
        assert!((end[0] - 150.0).abs() < 1e-6 && (end[1] - 40.0).abs() < 1e-6);
    }

    #[test]
    fn two_bone_clamps_unreachable() {
        let (a1, a2) = ik_two_bone(100.0, 80.0, [0.0, 0.0], [500.0, 0.0]);
        let pts = fk(&[100.0, 80.0], &[a1, a2], [0.0, 0.0]);
        let reach = (pts[2][0].powi(2) + pts[2][1].powi(2)).sqrt();
        assert!((reach - 180.0).abs() < 1e-3);
        assert!(pts[2][0] > 0.0);
    }

    #[test]
    fn ccd_converges_three_bone() {
        let lengths = [100.0, 80.0, 60.0];
        let angles = ik_ccd(&lengths, [0.0, 0.0], [150.0, 40.0], &[0.0, 0.0, 0.0], 64);
        let pts = fk(&lengths, &angles, [0.0, 0.0]);
        let end = pts[pts.len() - 1];
        assert!((end[0] - 150.0).hypot(end[1] - 40.0) < 1.0);
    }

    #[test]
    fn damp_and_spring_behave() {
        assert!((damp(0.0, 10.0, 5.0, 1.0) - 10.0).abs() < 0.1);
        assert_eq!(damp(3.0, 10.0, 0.0, 1.0), 3.0);
        let (p, _) = spring_step(0.0, 0.0, 10.0, 100.0, 10.0, 0.016);
        assert!(p > 0.0 && p < 10.0);
    }

    #[test]
    fn lengths_rejected() {
        assert!(check_lengths(&[]).is_err());
        assert!(check_lengths(&[1.0; 17]).is_err());
        assert!(check_lengths(&[-1.0]).is_err());
        assert!(check_lengths(&[f64::NAN]).is_err());
        assert!(check_lengths(&[10.0]).is_ok());
    }
}
