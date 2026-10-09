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

impl VbluaError {
    /// UI-facing hint per failure class (Phase 25 error experience).
    pub fn suggestion(&self) -> &'static str {
        match self {
            VbluaError::Syntax { .. } => "Check the reported line: unclosed bracket, string, or block.",
            VbluaError::Runtime { .. } => {
                "A value was nil or an id unknown. Log values before the failing call."
            }
            VbluaError::Api { .. } => {
                "Wrong argument type or unknown key — the message lists what fits."
            }
            VbluaError::Permission { .. } => {
                "The manifest/policy lacks this capability. Declare it or degrade gracefully."
            }
            VbluaError::Timeout { .. } => {
                "Loop without progress? Batch the work (vblua.batch) instead of per-item calls."
            }
            VbluaError::Resource { .. } => {
                "Lower the batch size or free large tables; budgets guard the host."
            }
            VbluaError::Internal(_) => "Binding bug — please report with the script attached.",
        }
    }

    /// Multi-line UI rendering: kind, detail (with interpreter line info
    /// when present), suggestion. The console pushes this verbatim.
    pub fn render(&self) -> String {
        let kind = match self {
            VbluaError::Syntax { .. } => "Syntax",
            VbluaError::Runtime { .. } => "Runtime",
            VbluaError::Api { .. } => "API",
            VbluaError::Permission { .. } => "Permission",
            VbluaError::Timeout { .. } => "Timeout",
            VbluaError::Resource { .. } => "Resource",
            VbluaError::Internal(_) => "Internal",
        };
        format!("VBLua Error [{kind}]\n{self}\nSuggestion: {}", self.suggestion())
    }
}

/// Classify an `mlua::Error` into [`VbluaError`] without losing the message.
pub fn from_mlua(script: &str, err: mlua::Error) -> VbluaError {
    use mlua::Error as E;
    let message = err.to_string();
    // Instruction-budget kills carry the sentinel (sandbox::arm_execution_limits).
    if message.contains(super::sandbox::TIMEOUT_SENTINEL) {
        // Extract the budget from "... (N instructions)".
        let budget = message
            .rsplit('(')
            .next()
            .and_then(|s| s.split_whitespace().next())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        return VbluaError::Timeout {
            script: script.to_string(),
            budget_instructions: budget,
        };
    }
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

    #[test]
    fn render_includes_kind_and_suggestion() {
        let e = VbluaError::Timeout {
            script: "a.lua".into(),
            budget_instructions: 7,
        };
        let r = e.render();
        assert!(r.contains("Timeout") && r.contains("Suggestion:") && r.contains("a.lua"));
    }
}
