use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ShadingStack {
    #[default]
    Behind,
    Middle,
    OnTop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadingPass {
    pub id: Uuid,
    pub name: String,
    /// WGSL fragment shader source (standard shading language for this editor).
    pub wgsl: String,
    #[serde(default)]
    pub uniforms: Vec<f32>,
    #[serde(default)]
    pub stack: ShadingStack,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

impl ShadingPass {
    pub fn new_preset(name: impl Into<String>, wgsl: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            wgsl: wgsl.into(),
            uniforms: Vec::new(),
            stack: ShadingStack::OnTop,
            enabled: true,
        }
    }

    pub fn crt_preset() -> Self {
        Self::new_preset(
            "CRT",
            include_str!("../shading/presets/crt.wgsl").to_string(),
        )
    }

    pub fn vignette_preset() -> Self {
        Self::new_preset(
            "Vignette",
            include_str!("../shading/presets/vignette.wgsl").to_string(),
        )
    }

    pub fn blackhole_preset() -> Self {
        let mut p = Self::new_preset(
            "Blackhole",
            include_str!("../shading/presets/blackhole.wgsl").to_string(),
        );
        // time, strength, disk_radius (see blackhole.wgsl Uniforms)
        p.uniforms = vec![0.0, 0.95, 0.22];
        p
    }
}