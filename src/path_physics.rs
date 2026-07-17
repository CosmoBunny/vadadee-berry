//! Fixed-timestep particle simulation for path weight-flow sculpting.
//!
//! **Why not Avian2D here:** `avian2d` is a Bevy ECS plugin and would pull a full Bevy
//! runtime into the editor. Path sculpting needs soft particle springs + custom brush
//! forces, which this lightweight integrator handles with better control and no ECS overhead.
//! The force model matches the design plan (shrink/expand/drag/magnetic + edge stiffness).

use glam::Vec2;

use crate::tools::weight_flow::{Falloff, MagneticPole, WeightFlowConfig, WeightFlowMode};

/// Soft-body path simulation (one particle per path anchor).
#[derive(Debug, Clone)]
pub struct PathPhysicsSim {
    pub positions: Vec<Vec2>,
    pub velocities: Vec<Vec2>,
    pub masses: Vec<f32>,
    /// Rest length of edge i → i+1 (and last→0 if closed).
    pub rest_lengths: Vec<f32>,
    pub closed: bool,
    pub lock_endpoints: bool,
}

impl PathPhysicsSim {
    pub fn from_anchors(
        anchors: &[(f64, f64)],
        closed: bool,
        point_mass: f32,
        lock_endpoints: bool,
    ) -> Self {
        let n = anchors.len();
        let positions: Vec<Vec2> = anchors
            .iter()
            .map(|&(x, y)| Vec2::new(x as f32, y as f32))
            .collect();
        let mut rest_lengths = Vec::new();
        if n >= 2 {
            let edge_count = if closed { n } else { n - 1 };
            for i in 0..edge_count {
                let j = (i + 1) % n;
                rest_lengths.push((positions[j] - positions[i]).length().max(1e-3));
            }
        }
        let mass = point_mass.max(0.05);
        Self {
            positions,
            velocities: vec![Vec2::ZERO; n],
            masses: vec![mass; n],
            rest_lengths,
            closed,
            lock_endpoints,
        }
    }

    pub fn to_anchors(&self) -> Vec<(f64, f64)> {
        self.positions
            .iter()
            .map(|p| (p.x as f64, p.y as f64))
            .collect()
    }

    /// Integrate `substeps` of size `dt` under brush influence.
    pub fn step(
        &mut self,
        brush: Vec2,
        brush_vel: Vec2,
        mode: WeightFlowMode,
        cfg: &WeightFlowConfig,
        dt: f32,
        substeps: u32,
    ) {
        let n = self.positions.len();
        if n < 2 {
            return;
        }
        let h = (dt / substeps.max(1) as f32).clamp(1e-4, 1.0 / 30.0);
        let radius = cfg.radius.max(1.0);
        let strength = cfg.strength.clamp(0.0, 4.0);
        let stiff = cfg.stiffness.clamp(0.0, 1.0);
        let damp = cfg.damping.clamp(0.0, 1.0);

        for _ in 0..substeps.max(1) {
            let mut forces = vec![Vec2::ZERO; n];

            // Edge springs (straight / length preserve)
            let edge_count = if self.closed { n } else { n.saturating_sub(1) };
            let k_spring = 80.0 * stiff + 8.0; // N/m scale in doc-px units
            for i in 0..edge_count {
                let j = (i + 1) % n;
                let delta = self.positions[j] - self.positions[i];
                let dist = delta.length().max(1e-4);
                let rest = self.rest_lengths.get(i).copied().unwrap_or(dist);
                let dir = delta / dist;
                let f = dir * (dist - rest) * k_spring;
                forces[i] += f;
                forces[j] -= f;
            }

            // Brush forces
            match mode {
                WeightFlowMode::Shrink | WeightFlowMode::Expand => {
                    let attract = matches!(mode, WeightFlowMode::Shrink);
                    // Pairwise within brush
                    for i in 0..n {
                        let di = (self.positions[i] - brush).length();
                        if di > radius {
                            continue;
                        }
                        let wi = falloff(di, radius, cfg.falloff) * strength;
                        // Toward / away from brush center
                        if di > 1e-3 {
                            let to_brush = (brush - self.positions[i]) / di;
                            let center_f = if attract { to_brush } else { -to_brush };
                            forces[i] += center_f * (wi * 120.0);
                        }
                        for j in (i + 1)..n {
                            let dj = (self.positions[j] - brush).length();
                            if dj > radius {
                                continue;
                            }
                            let wj = falloff(dj, radius, cfg.falloff) * strength;
                            let pair_w = wi.min(wj);
                            let delta = self.positions[j] - self.positions[i];
                            let dist = delta.length().max(1e-3);
                            let dir = delta / dist;
                            let mag = pair_w * 90.0;
                            if attract {
                                forces[i] += dir * mag;
                                forces[j] -= dir * mag;
                            } else {
                                forces[i] -= dir * mag;
                                forces[j] += dir * mag;
                            }
                        }
                    }
                }
                WeightFlowMode::Drag => {
                    let hit_r = (radius * 0.55).max(4.0);
                    let speed = brush_vel.length();
                    for i in 0..n {
                        let d = (self.positions[i] - brush).length();
                        if d > hit_r {
                            continue;
                        }
                        let w = falloff(d, hit_r, cfg.falloff) * strength;
                        // Pull toward brush + carry velocity (force ∝ speed)
                        let to_brush = if d > 1e-3 {
                            (brush - self.positions[i]) / d
                        } else {
                            Vec2::ZERO
                        };
                        forces[i] += to_brush * (w * 200.0);
                        forces[i] += brush_vel * (w * (8.0 + speed * 0.05));
                    }
                }
                WeightFlowMode::Magnetic => {
                    // Path anchors = South poles (−1).
                    // Brush North (+1): opposite → attract (pull points in).
                    // Brush South (−1): same → repel (push points out).
                    let q_brush = match cfg.magnetic_pole {
                        MagneticPole::North => 1.0f32,
                        MagneticPole::South => -1.0f32,
                    };
                    let q_point = -1.0f32;
                    // Soft core so force doesn't explode at d→0, but still feels 1/r².
                    let soft = (radius * 0.08).max(2.0);
                    let k = 12_000.0 * strength.max(0.05);
                    for i in 0..n {
                        let delta = self.positions[i] - brush; // brush → point
                        let d = delta.length();
                        if d > radius || d < 1e-5 {
                            continue;
                        }
                        // Edge fade only in outer 25% of radius — core is pure magnetic.
                        let edge = (radius * 0.75).max(1.0);
                        let edge_w = if d <= edge {
                            1.0
                        } else {
                            let t = 1.0 - (d - edge) / (radius - edge).max(1e-3);
                            t.clamp(0.0, 1.0).powi(2)
                        };
                        let dir = delta / d;
                        // Coulomb: F = k * q1*q2 / (d² + soft²) * r̂
                        // Negative product → force toward brush (attract).
                        let inv = 1.0 / (d * d + soft * soft);
                        let coulomb = k * q_brush * q_point * inv;
                        let mut f = dir * (coulomb * edge_w);
                        // Extra near-field snap for N (attract) when very close — feels "magnetic".
                        if q_brush > 0.0 && d < radius * 0.35 {
                            let snap = (1.0 - d / (radius * 0.35)).clamp(0.0, 1.0);
                            f += -dir * (snap * strength * 180.0);
                        }
                        // Same-pole: stronger kick when close (like magnets flipping away).
                        if q_brush < 0.0 && d < radius * 0.4 {
                            let kick = (1.0 - d / (radius * 0.4)).clamp(0.0, 1.0);
                            f += dir * (kick * strength * 220.0);
                        }
                        let max_f = 900.0 * strength.max(0.15);
                        forces[i] += f.clamp_length_max(max_f);
                        // Light damping of relative motion so field feels sticky, not floaty.
                        let radial_v = self.velocities[i].dot(dir);
                        forces[i] -= dir * (radial_v * 12.0 * edge_w);
                    }
                }
            }

            // Integrate
            for i in 0..n {
                if self.lock_endpoints && !self.closed && (i == 0 || i + 1 == n) {
                    self.velocities[i] = Vec2::ZERO;
                    continue;
                }
                let inv_m = 1.0 / self.masses[i].max(0.05);
                self.velocities[i] += forces[i] * inv_m * h;
                // damping
                self.velocities[i] *= (1.0 - damp * 0.35).clamp(0.5, 1.0);
                // speed clamp
                self.velocities[i] = self.velocities[i].clamp_length_max(800.0);
                self.positions[i] += self.velocities[i] * h;
            }
        }
    }
}

fn falloff(d: f32, radius: f32, kind: Falloff) -> f32 {
    let t = (1.0 - (d / radius.max(1e-3)).clamp(0.0, 1.0)).max(0.0);
    match kind {
        Falloff::Hard => {
            if d <= radius {
                1.0
            } else {
                0.0
            }
        }
        Falloff::Linear => t,
        Falloff::Smooth => t * t * (3.0 - 2.0 * t), // smoothstep
    }
}
