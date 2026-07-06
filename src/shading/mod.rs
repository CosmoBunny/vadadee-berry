//! WGSL shading passes composited over the canvas.

pub mod cpu_effects;
pub mod procedural_blackhole;
pub mod wgpu_pass;

pub use cpu_effects::draw_shading_passes;
pub use wgpu_pass::{
    init_callback_resources, queue_shading_input, shading_passes_need_input, ShadingRenderer,
};