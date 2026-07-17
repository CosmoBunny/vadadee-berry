//! Weight flow brush — sculpt path anchors with soft forces (edit mode).

use crate::document::NodeId;
use crate::path_physics::PathPhysicsSim;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WeightFlowMode {
    #[default]
    Shrink,
    Expand,
    Drag,
    Magnetic,
}

impl WeightFlowMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Shrink => "Shrink",
            Self::Expand => "Expand",
            Self::Drag => "Drag",
            Self::Magnetic => "Magnetic",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Falloff {
    #[default]
    Smooth,
    Linear,
    Hard,
}

impl Falloff {
    pub fn label(self) -> &'static str {
        match self {
            Self::Smooth => "Smooth",
            Self::Linear => "Linear",
            Self::Hard => "Hard",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MagneticPole {
    #[default]
    North,
    South,
}

impl MagneticPole {
    pub fn label(self) -> &'static str {
        match self {
            Self::North => "North (N)",
            Self::South => "South (S)",
        }
    }
}

#[derive(Debug, Clone)]
pub struct WeightFlowConfig {
    pub mode: WeightFlowMode,
    pub radius: f32,
    pub strength: f32,
    pub falloff: Falloff,
    pub point_mass: f32,
    /// Edge spring strength 0…1 — how hard segments keep spacing (“edge hold”).
    pub stiffness: f32,
    pub damping: f32,
    pub magnetic_pole: MagneticPole,
    pub lock_endpoints: bool,
    pub preserve_closed: bool,
}

impl Default for WeightFlowConfig {
    fn default() -> Self {
        Self {
            mode: WeightFlowMode::Shrink,
            radius: 40.0,
            strength: 0.55,
            falloff: Falloff::Smooth,
            point_mass: 1.0,
            stiffness: 0.5,
            damping: 0.45,
            magnetic_pole: MagneticPole::North,
            lock_endpoints: true,
            preserve_closed: true,
        }
    }
}

/// Active sculpt stroke (one undo on release).
#[derive(Debug, Clone)]
pub struct WeightFlowStroke {
    pub node_id: NodeId,
    /// Full node snapshot for undo.
    pub before: crate::document::Node,
    pub sim: PathPhysicsSim,
    pub brush_prev: Option<glam::Vec2>,
    pub brush_vel: glam::Vec2,
}

#[derive(Debug, Clone, Default)]
pub struct WeightFlowBrush {
    /// Geometry tab toggle — default **off**.
    pub enabled: bool,
    pub config: WeightFlowConfig,
    pub stroke: Option<WeightFlowStroke>,
    /// Last cursor in doc space (for overlay when idle).
    pub cursor_doc: Option<(f64, f64)>,
}

impl WeightFlowBrush {
    pub fn is_active(&self) -> bool {
        self.enabled
    }

    pub fn cancel_stroke(&mut self) {
        self.stroke = None;
    }
}
