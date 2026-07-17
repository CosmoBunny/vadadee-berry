//! WGSL shading passes composited over the canvas.

pub mod cpu_effects;
pub mod cpu_hex_export;
pub mod graph_blur;
pub mod procedural_blackhole;
pub mod wgpu_pass;

pub use cpu_effects::draw_shading_passes;
pub use wgpu_pass::{
    composite_shading_layers_into_rgba, init_callback_resources, probe_compile_shading_wgsl,
    queue_shading_input, render_shading_pass_to_rgba, shading_passes_need_input,
    validate_shading_wgsl, ShadingRenderer,
};

/// Load WGSL text from a filesystem path (desktop / host tooling).
/// Does not compile; GPU validation runs when the pass is applied.
pub fn load_wgsl_file(path: &std::path::Path) -> Result<String, String> {
    let src = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    if src.trim().is_empty() {
        return Err("WGSL file is empty".into());
    }
    Ok(src)
}

/// Write WGSL text to disk.
pub fn save_wgsl_file(path: &std::path::Path, source: &str) -> Result<(), String> {
    std::fs::write(path, source).map_err(|e| format!("Failed to write {}: {e}", path.display()))
}