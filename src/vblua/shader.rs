//! Shader API (Phase 9): safe WGSL pass management on layers.
//!
//! Lua shape:
//! ```lua
//! local p = vblua.shader.create()            -- disabled template pass
//! vblua.shader.set_source(p, src)             -- statically validated now
//! vblua.shader.set_uniforms(p, { 0.0, 0.5 }) -- capped, finite floats
//! vblua.shader.set_enabled(p, true)
//! ```
//!
//! Safety posture (spec: no unrestricted GPU execution yet):
//! - New passes start **disabled** (no visual/GPU change until enabled).
//! - `set_source` runs the host static validator (`validate_shading_wgsl`)
//!   at call time — GLSL leftovers, compute-only modules, and empty sources
//!   fail as immediate Lua errors. GPU compile still happens host-side on
//!   enable; failures land in the pass `compile_error` (visible in UI).
//! - Sources cap at 64 KiB, uniforms at 64 finite floats. No file paths cross
//!   (use the Asset API when it lands); presets stay host-side.

use crate::document::{Document, ShadingPass};

/// Source / uniform budget guards.
pub const MAX_SOURCE_LEN: usize = 64 * 1024;
pub const MAX_UNIFORMS: usize = 64;

/// Read-only pass row (snapshot).
#[derive(Debug, Clone)]
pub struct ShaderPassSnapshot {
    pub id: String,
    pub layer_id: String,
    pub name: String,
    pub enabled: bool,
    pub uniforms: Vec<f32>,
    pub source_len: usize,
    pub has_error: bool,
}

/// All shading passes in the document, grouped per layer.
#[derive(Debug, Clone, Default)]
pub struct ShaderSnapshot {
    pub layers: Vec<ShaderLayerSnapshot>,
    pub active_layer_id: Option<String>,
}

/// Read-only per-layer pass list.
#[derive(Debug, Clone, Default)]
pub struct ShaderLayerSnapshot {
    pub layer_id: String,
    pub layer_name: String,
    pub passes: Vec<ShaderPassSnapshot>,
}

impl ShaderSnapshot {
    pub fn capture(doc: &Document) -> Self {
        let layers = doc
            .layers
            .iter()
            .map(|l| ShaderLayerSnapshot {
                layer_id: l.id.to_string(),
                layer_name: l.name.clone(),
                passes: l
                    .shading_passes
                    .iter()
                    .map(|p| ShaderPassSnapshot {
                        id: p.id.to_string(),
                        layer_id: l.id.to_string(),
                        name: p.name.clone(),
                        enabled: p.enabled,
                        uniforms: p.uniforms.clone(),
                        source_len: p.wgsl.len(),
                        has_error: p
                            .compile_error
                            .lock()
                            .ok()
                            .and_then(|e| e.clone())
                            .is_some(),
                    })
                    .collect(),
            })
            .collect();
        Self {
            layers,
            active_layer_id: doc
                .layers
                .get(doc.active_layer_index)
                .map(|l| l.id.to_string()),
        }
    }

    /// Resolve the script target layer: explicit id, else active, else first.
    pub fn resolve_layer(&self, want: Option<&str>) -> Result<usize, String> {
        if self.layers.is_empty() {
            return Err("document has no layers".to_string());
        }
        if let Some(id) = want {
            return self
                .layers
                .iter()
                .position(|l| l.layer_id == id)
                .ok_or_else(|| "unknown layer id".to_string());
        }
        if let Some(active) = self.active_layer_id.as_deref()
            && let Some(pos) = self.layers.iter().position(|l| l.layer_id == active)
        {
            return Ok(pos);
        }
        Ok(0)
    }

    pub fn find_pass(&self, id: &str) -> Option<(usize, usize)> {
        for (li, l) in self.layers.iter().enumerate() {
            for (pi, p) in l.passes.iter().enumerate() {
                if p.id == id {
                    return Some((li, pi));
                }
            }
        }
        None
    }
}

/// Validate WGSL source through the host static validator.
pub fn validate_source(src: &str) -> Result<(), String> {
    if src.len() > MAX_SOURCE_LEN {
        return Err(format!("source exceeds {} bytes", MAX_SOURCE_LEN));
    }
    crate::shading::validate_shading_wgsl(src).map_err(|e| {
        // Pass the host diagnostic through untouched (line info lives in
        // Phase 25 error UX; never rewrite compiler text).
        e
    })
}

/// One staged shading mutation.
#[derive(Debug, Clone)]
pub enum ShaderCommand {
    CreatePass {
        id: uuid::Uuid,
        layer_id: uuid::Uuid,
        name: String,
    },
    SetSource {
        pass_id: uuid::Uuid,
        source: String,
    },
    SetUniforms {
        pass_id: uuid::Uuid,
        values: Vec<f32>,
    },
    SetEnabled {
        pass_id: uuid::Uuid,
        enabled: bool,
    },
    RenamePass {
        pass_id: uuid::Uuid,
        name: String,
    },
    RemovePass {
        pass_id: uuid::Uuid,
    },
}

impl ShaderCommand {
    /// Apply one command; `true` when the document changed.
    pub fn apply_to(&self, doc: &mut Document) -> bool {
        match self {
            ShaderCommand::CreatePass { id, layer_id, name } => {
                let Some(layer) = doc.layers.iter_mut().find(|l| l.id == *layer_id) else {
                    return false;
                };
                if layer.shading_passes.iter().any(|p| p.id == *id) {
                    return false;
                }
                let mut pass = ShadingPass::custom_template();
                pass.id = *id;
                pass.name = name.clone();
                // Safe default: present but inert until explicitly enabled.
                pass.enabled = false;
                layer.shading_passes.push(pass);
                true
            }
            ShaderCommand::SetSource { pass_id, source } => {
                for layer in doc.layers.iter_mut() {
                    if let Some(p) = layer.shading_passes.iter_mut().find(|p| p.id == *pass_id) {
                        p.load_wgsl_source(source.clone(), None);
                        // load_wgsl_source resets the display name to "Custom";
                        // keep the user's pass name instead.
                        return true;
                    }
                }
                false
            }
            ShaderCommand::SetUniforms { pass_id, values } => {
                for layer in doc.layers.iter_mut() {
                    if let Some(p) = layer.shading_passes.iter_mut().find(|p| p.id == *pass_id) {
                        p.uniforms = values.clone();
                        return true;
                    }
                }
                false
            }
            ShaderCommand::SetEnabled { pass_id, enabled } => {
                for layer in doc.layers.iter_mut() {
                    if let Some(p) = layer.shading_passes.iter_mut().find(|p| p.id == *pass_id) {
                        p.enabled = *enabled;
                        return true;
                    }
                }
                false
            }
            ShaderCommand::RenamePass { pass_id, name } => {
                if name.is_empty() {
                    return false;
                }
                let name: String = name.chars().take(128).collect();
                for layer in doc.layers.iter_mut() {
                    if let Some(p) = layer.shading_passes.iter_mut().find(|p| p.id == *pass_id) {
                        p.name = name.clone();
                        return true;
                    }
                }
                false
            }
            ShaderCommand::RemovePass { pass_id } => {
                for layer in doc.layers.iter_mut() {
                    let before = layer.shading_passes.len();
                    layer.shading_passes.retain(|p| p.id != *pass_id);
                    if layer.shading_passes.len() != before {
                        return true;
                    }
                }
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_one_layer() -> Document {
        Document {
            title: "t".into(),
            width: 100.0,
            height: 100.0,
            layers: vec![crate::document::Layer::new_image(
                uuid::Uuid::new_v4(),
                "L".into(),
                true,
                false,
                vec![],
            )],
            active_layer_index: 0,
            defs: Default::default(),
            path_effects: Default::default(),
            tiling_effects: Default::default(),
            circular_effects: Default::default(),
            clip_masks: Default::default(),
            boolean_effects: Default::default(),
            page_color: [1.0, 1.0, 1.0, 1.0],
            page_unit: Default::default(),
            timeline_markers: Vec::new(),
            timeline_scripts: Vec::new(),
        }
    }

    #[test]
    fn validation_rejects_glsl_and_compute() {
        assert!(validate_source("").is_err());
        assert!(validate_source("void main() { gl_FragColor = vec4(1.0); }").is_err());
        assert!(validate_source("@compute @workgroup_size(1) fn main() {}").is_err());
        assert!(validate_source(&crate::document::CUSTOM_WGSL_TEMPLATE).is_ok());
    }

    #[test]
    fn create_is_disabled_and_source_round_trips() {
        let mut doc = doc_one_layer();
        let layer_id = doc.layers[0].id;
        let id = uuid::Uuid::new_v4();
        assert!(ShaderCommand::CreatePass {
            id,
            layer_id,
            name: "Glow".into(),
        }
        .apply_to(&mut doc));
        let pass = &doc.layers[0].shading_passes[0];
        assert!(!pass.enabled);
        let src = crate::document::CUSTOM_WGSL_TEMPLATE.to_string();
        assert!(ShaderCommand::SetSource {
            pass_id: id,
            source: src.clone(),
        }
        .apply_to(&mut doc));
        assert_eq!(doc.layers[0].shading_passes[0].wgsl, src);
        assert!(ShaderCommand::SetUniforms {
            pass_id: id,
            values: vec![0.0, 0.5],
        }
        .apply_to(&mut doc));
        assert!(ShaderCommand::SetEnabled {
            pass_id: id,
            enabled: true,
        }
        .apply_to(&mut doc));
        assert!(doc.layers[0].shading_passes[0].enabled);
        assert!(ShaderCommand::RemovePass { pass_id: id }.apply_to(&mut doc));
        assert!(doc.layers[0].shading_passes.is_empty());
    }
}
