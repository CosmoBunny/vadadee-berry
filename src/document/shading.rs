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

/// Starter fragment module for Custom / file-loaded shaders.
/// Bindings: compose = input_tex@0, sampler@1, uniform@2.
/// Runtime fills uniforms[0] += time, uniforms[3] = page aspect.
pub const CUSTOM_WGSL_TEMPLATE: &str = r#"// Custom shading pass (fragment, not compute).
// Required entry: @fragment fn main(...) -> @location(0) vec4<f32>
// Compose bindings (when using input_tex):
//   @group(0) @binding(0) texture_2d  input_tex
//   @group(0) @binding(1) sampler     input_sampler
//   @group(0) @binding(2) uniform     u
// Procedural only (no input_tex): put uniform at @binding(0).
// Runtime: u[0] gets +time, u[3] is page aspect (ensure at least 4 floats).

@group(0) @binding(0) var input_tex: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;

struct Uniforms {
    time: f32,
    strength: f32,
    _pad2: f32,
    aspect: f32,
}
@group(0) @binding(2) var<uniform> u: Uniforms;

@fragment
fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let c = textureSample(input_tex, input_sampler, uv);
    let d = distance(uv, vec2(0.5));
    let vig = smoothstep(0.9, 0.35 * (1.0 - u.strength), d);
    return vec4(c.rgb * vig, c.a);
}
"#;

impl ShadingPass {
    pub fn new_preset(name: impl Into<String>, wgsl: impl Into<String>) -> Self {
        let wgsl = wgsl.into();
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            wgsl: wgsl.clone(),
            uniforms: Vec::new(),
            stack: ShadingStack::OnTop,
            enabled: true,
            compile_error: Arc::new(Mutex::new(None)),
            hot_reload: true,
            // Compile immediately so GPU path sees the source without waiting for Hot toggle.
            compiled_wgsl: Some(wgsl),
        }
    }

    /// Editable starter for user shaders (not a fixed visual preset).
    pub fn custom_template() -> Self {
        let mut p = Self::new_preset("Custom", CUSTOM_WGSL_TEMPLATE);
        p.uniforms = vec![0.0, 0.5, 0.0];
        p
    }

    /// Replace source with arbitrary WGSL (file load / paste / MCP). Marks as Custom and arms compile.
    pub fn load_wgsl_source(&mut self, source: impl Into<String>, display_name: Option<&str>) {
        let src = source.into();
        self.name = display_name
            .map(|s| s.to_string())
            .unwrap_or_else(|| "Custom".to_string());
        self.wgsl = src.clone();
        self.compiled_wgsl = Some(src);
        if let Ok(mut err) = self.compile_error.lock() {
            *err = None;
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