//! VBLua — Vadadee Berry's programmable application platform.
//!
//! Architecture (never violated):
//! ```text
//! Lua -> VBLua API -> Rust abstraction -> platform implementation
//! ```
//! Lua never touches Rust internals, GPU resources, the filesystem, or
//! platform APIs directly. Mutations cross as batched [`context::DocumentCommand`]s
//! so one script run can become one undo transaction.
//!
//! Status: Phases 1-4 (foundation, lifecycle, stdlib, document API).
//! Upcoming: node graph (5-6), animation (7), assets/picker (11-12), UI (13),
//! sandbox manifests + addons (17-19), mobile validation (22-23).

mod api;
mod context;
mod error;
mod runtime;
mod sandbox;
mod value;

pub use api::ApiHost;
pub use context::{DocumentCommand, DocumentSnapshot, LayerInfo};
pub use error::{VbluaError, from_mlua};
pub use runtime::{Lifecycle, VbRuntime};
pub use sandbox::{Capability, SandboxPolicy};
pub use value::VbValue;

/// VBLua implementation version (the `vblua.version()` string).
pub const VBLUA_VERSION: &str = "0.1.0";
/// Script-facing API version (`vblua.api_version()`, manifests pin this).
pub const API_VERSION: &str = "1";
