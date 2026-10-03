//! VBLua errors — every Lua failure becomes a structured Rust error.
//!
//! Lua must never crash the host. All script failures surface as
//! [`VbluaError`] so the UI can render them (Phase 25) instead of panicking.

use thiserror::Error;

/// Structured VBLua failure (Phase 2 + Phase 25 groundwork).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum VbluaError {
    /// `load()` rejected the chunk (syntax error). Holds the interpreter message.
    #[error("syntax error in {script}: {message}")]
    Syntax { script: String, message: String },
    /// The chunk ran but raised. Holds the interpreter message.
    #[error("runtime error in {script}: {message}")]
    Runtime { script: String, message: String },
    /// A `vblua.*` function was misused (bad args, unknown id, ...).
    #[error("API error in {api}: {message}")]
    Api { api: String, message: String },
    /// The sandbox denied a capability (filesystem, network, ...).
    #[error("permission denied ({capability}): {message}")]
    Permission { capability: String, message: String },
    /// Execution budget exhausted (instruction / time limit).
    #[error("script timeout in {script} after {budget_instructions} instructions")]
    Timeout {
        script: String,
        budget_instructions: u64,
    },
    /// A host resource limit was hit (output too large, too many commands, ...).
    #[error("resource limit ({resource}): {message}")]
    Resource { resource: String, message: String },
    /// Bug in the VBLua binding itself — never a script authoring error.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Classify an `mlua::Error` into [`VbluaError`] without losing the message.
pub fn from_mlua(script: &str, err: mlua::Error) -> VbluaError {
    use mlua::Error as E;
    let message = err.to_string();
    match err {
        E::SyntaxError { .. } => VbluaError::Syntax {
            script: script.to_string(),
            message,
        },
        E::RuntimeError(_) => VbluaError::Runtime {
            script: script.to_string(),
            message,
        },
        E::BadArgument { .. } | E::FromLuaConversionError { .. } | E::ToLuaConversionError { .. } => {
            VbluaError::Api {
                api: script.to_string(),
                message,
            }
        }
        E::MemoryError(_) => VbluaError::Resource {
            resource: "memory".into(),
            message,
        },
        other => VbluaError::Runtime {
            script: script.to_string(),
            message: other.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syntax_maps_to_syntax() {
        let err = mlua::Error::SyntaxError {
            message: "boom".into(),
            incomplete_input: false,
        };
        assert!(matches!(
            from_mlua("a.lua", err),
            VbluaError::Syntax { .. }
        ));
    }

    #[test]
    fn runtime_maps_to_runtime() {
        let err = mlua::Error::RuntimeError("boom".into());
        assert!(matches!(
            from_mlua("a.lua", err),
            VbluaError::Runtime { .. }
        ));
    }
}
