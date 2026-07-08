use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ShadingStack {
    #[default]
    Behind,
    Middle,
    OnTop,
}

#[derive(Debug, Serialize, Deserialize)]
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

    #[serde(skip, default = "default_compile_error")]
    pub compile_error: Arc<Mutex<Option<String>>>,
    #[serde(default = "default_hot_reload")]
    pub hot_reload: bool,
    #[serde(skip)]
    pub compiled_wgsl: Option<String>,
}

impl Clone for ShadingPass {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            name: self.name.clone(),
            wgsl: self.wgsl.clone(),
            uniforms: self.uniforms.clone(),
            stack: self.stack,
            enabled: self.enabled,
            compile_error: Arc::new(Mutex::new(self.compile_error.lock().unwrap().clone())),
            hot_reload: self.hot_reload,
            compiled_wgsl: self.compiled_wgsl.clone(),
        }
    }
}

fn default_enabled() -> bool {
    true
}

fn default_compile_error() -> Arc<Mutex<Option<String>>> {
    Arc::new(Mutex::new(None))
}

fn default_hot_reload() -> bool {
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
            compile_error: Arc::new(Mutex::new(None)),
            hot_reload: true,
            compiled_wgsl: None,
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

    /// Pure twinkling starfield (no GPU WGSL required — CPU-rendered by name match).
    pub fn starfield_preset() -> Self {
        let mut p = Self::new_preset(
            "Starfield",
            // Minimal stub so `pass.wgsl` is non-empty; the CPU path detects the name.
            "// Starfield — rendered via CPU starfield path.".to_string(),
        );
        // uniforms[0]: time offset (seconds). Useful for animation offset.
        p.uniforms = vec![0.0];
        p.stack = ShadingStack::Behind;
        p
    }
}