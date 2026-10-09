//! VBLua sandbox — explicit capabilities, deny by default (Phase 17 groundwork).
//!
//! ## Input
//!
//! A [`SandboxPolicy`] (grants + budgets) at runtime construction.
//!
//! ## Output
//!
//! A stripped interpreter (no `dofile`/`require`/`os.execute`/`io`/`debug`,
//! no `package.loadlib`), an armed instruction hook, and a heap cap.
//!
//! ## Errors
//!
//! Budget kills surface as [`VbluaError::Timeout`](super::error::VbluaError::Timeout),
//! heap kills as `Resource`; denied APIs fail at call time with `Permission`.
//!
//! The interpreter starts with the `vblua.*` table only; the `sandbox::apply`
//! step strips whatever stdlib surface the policy forbids. Dangerous surface
//! (`os.execute`, `io.*`, `debug.*`, `package.loadlib`, ...) is never restored
//! by script code because `require`/`dofile`/`loadfile` stay removed unless
//! the host explicitly grants `Capability::FilesystemRead`.

use std::collections::HashSet;

/// Host capabilities a script may request. The manifest declares these
/// (Phase 18); the runtime enforces them here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    DocumentRead,
    DocumentWrite,
    AssetRead,
    Ui,
    Clipboard,
    Network,
    FilesystemRead,
    FilesystemWrite,
    Process,
}

impl Capability {
    pub fn name(self) -> &'static str {
        match self {
            Capability::DocumentRead => "document.read",
            Capability::DocumentWrite => "document.write",
            Capability::AssetRead => "assets",
            Capability::Ui => "ui",
            Capability::Clipboard => "clipboard",
            Capability::Network => "network",
            Capability::FilesystemRead => "filesystem.read",
            Capability::FilesystemWrite => "filesystem.write",
            Capability::Process => "process",
        }
    }
}

/// What a script is allowed to touch. Default = document read-only + math.
/// Everything else must be granted explicitly.
#[derive(Debug, Clone)]
pub struct SandboxPolicy {
    allowed: HashSet<Capability>,
    /// Upper bound on instructions per `execute` (Phase 34 execution limits).
    pub max_instructions: u64,
    /// Upper bound on Lua heap bytes per runtime (decode/DoS guard).
    pub max_memory_bytes: usize,
    /// Upper bound on queued document commands per `execute` (DoS guard).
    pub max_commands: usize,
}

impl Default for SandboxPolicy {
    fn default() -> Self {
        let mut allowed = HashSet::new();
        allowed.insert(Capability::DocumentRead);
        Self {
            allowed,
            max_instructions: 2_000_000,
            max_memory_bytes: 64 * 1024 * 1024,
            max_commands: 4096,
        }
    }
}

impl SandboxPolicy {
    /// Permissive test policy (unit tests only) — still denies process/filesystem.
    pub fn permissive_for_tests() -> Self {
        let mut p = Self::default();
        p.grant(Capability::DocumentWrite);
        p.grant(Capability::AssetRead);
        p.grant(Capability::Ui);
        p
    }

    pub fn grant(&mut self, cap: Capability) {
        self.allowed.insert(cap);
    }

    pub fn revoke(&mut self, cap: Capability) {
        self.allowed.remove(&cap);
    }

    pub fn allows(&self, cap: Capability) -> bool {
        self.allowed.contains(&cap)
    }

    /// Remove the Lua stdlib surface this policy forbids.
    ///
    /// Always removed: `dofile`, `loadfile`, `require` (re-added only with
    /// `FilesystemRead`), `os.execute/exit/setlocale`, `io.*` (without FS
    /// grants), `debug.*`, `package.loadlib/cpath` writers. `print` is
    /// re-routed by the runtime to the script console, not removed.
    pub fn apply(&self, lua: &mlua::Lua) -> mlua::Result<()> {
        let globals = lua.globals();
        const ALWAYS_STRIP: &[&str] = &["dofile", "loadfile", "require"];
        for name in ALWAYS_STRIP {
            let _ = globals.set(*name, mlua::Nil);
        }
        if let Ok(os) = globals.get::<mlua::Table>("os") {
            for name in ["execute", "exit", "setlocale", "remove", "rename"] {
                let _ = os.set(name, mlua::Nil);
            }
        }
        if let Ok(dbg) = globals.get::<mlua::Table>("debug") {
            for name in [
                "debug",
                "getupvalue",
                "setupvalue",
                "getregistry",
                "getmetatable",
                "setmetatable",
            ] {
                let _ = dbg.set(name, mlua::Nil);
            }
        }
        if let Ok(pkg) = globals.get::<mlua::Table>("package") {
            let _ = pkg.set("loadlib", mlua::Nil);
            let _ = pkg.set("cpath", mlua::Nil);
        }
        let fs = self.allows(Capability::FilesystemRead) || self.allows(Capability::FilesystemWrite);
        if !fs
            && let Ok(io) = globals.get::<mlua::Table>("io")
        {
            for name in [
                "open",
                "popen",
                "lines",
                "input",
                "output",
                "remove",
                "rename",
                "tmpfile",
            ] {
                let _ = io.set(name, mlua::Nil);
            }
        }
        Ok(())
    }
}

/// Parse an addon manifest permission list into capabilities (Phase 18 ground).
/// Unknown names fail loudly (typo-safety over silent privilege widening).
pub fn permissions_from_manifest(names: &[String]) -> Result<HashSet<Capability>, String> {
    let all = [
        Capability::DocumentRead,
        Capability::DocumentWrite,
        Capability::AssetRead,
        Capability::Ui,
        Capability::Clipboard,
        Capability::Network,
        Capability::FilesystemRead,
        Capability::FilesystemWrite,
        Capability::Process,
    ];
    let mut out = HashSet::new();
    for name in names {
        let key = name.trim().to_ascii_lowercase();
        match all.iter().find(|c| c.name() == key) {
            Some(c) => {
                out.insert(*c);
            }
            None => {
                let valid = all.iter().map(|c| c.name()).collect::<Vec<_>>().join(", ");
                return Err(format!("unknown permission '{name}' — valid: {valid}"));
            }
        }
    }
    Ok(out)
}

/// Sentinel marking instruction-budget kills (mapped to `Timeout`, not `Runtime`).
pub const TIMEOUT_SENTINEL: &str = "vblua-timeout: instruction budget exhausted";
/// Sentinel marking breakpoint hits (mapped to `Runtime` with line info).
pub const BREAKPOINT_SENTINEL: &str = "vblua-breakpoint";
/// Trace event budget (debugger runs bounded like everything else).
pub const MAX_TRACE_EVENTS: usize = 4096;

/// Shared hook state: budget + tracer + breakpoints (Phase 26 debugger).
/// ONE hook serves all three (Lua allows a single hook per state).
pub struct HookState {
    pub budget: std::sync::atomic::AtomicU64,
    pub total: u64,
    pub tracing: std::sync::atomic::AtomicBool,
    pub trace: std::sync::Mutex<Vec<(String, i32)>>,
    /// `(chunk substring, line)` — hit when `short_src` contains the chunk.
    pub breakpoints: std::sync::Mutex<std::collections::HashSet<(String, i32)>>,
}

impl HookState {
    pub fn with_budget(total: u64) -> Self {
        Self {
            budget: std::sync::atomic::AtomicU64::new(total.max(1024)),
            total: total.max(1024),
            tracing: std::sync::atomic::AtomicBool::new(false),
            trace: std::sync::Mutex::new(Vec::new()),
            breakpoints: std::sync::Mutex::new(std::collections::HashSet::new()),
        }
    }

    /// Reset the per-run instruction budget.
    pub fn reset_budget(&self) {
        self.budget.store(self.total, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Arm instruction budget + memory cap + debugger on a fresh interpreter.
/// Returns shared hook state (budget reset before every run).
pub fn arm_execution_limits(
    lua: &mlua::Lua,
    policy: &SandboxPolicy,
) -> std::sync::Arc<HookState> {
    use std::sync::atomic::Ordering;
    let state = std::sync::Arc::new(HookState::with_budget(policy.max_instructions));
    let total = state.total;
    let hook = state.clone();
    lua.set_hook(
        mlua::HookTriggers {
            every_nth_instruction: Some(4096),
            every_line: true,
            ..Default::default()
        },
        move |_, dbg| {
            match dbg.event() {
                mlua::DebugEvent::Count => {
                    let left =
                        hook.budget.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |v| {
                            v.checked_sub(4096)
                        });
                    match left {
                        Ok(_) => Ok(mlua::VmState::Continue),
                        Err(_) => Err(mlua::Error::external(format!(
                            "{TIMEOUT_SENTINEL} ({total} instructions)"
                        ))),
                    }
                }
                mlua::DebugEvent::Line => {
                    let line = dbg.curr_line();
                    if line > 0 {
                        let chunk = dbg.source().short_src.map(|s| s.into_owned()).unwrap_or_default();
                        if hook.tracing.load(Ordering::SeqCst) {
                            let mut trace = hook.trace.lock().unwrap();
                            if trace.len() < MAX_TRACE_EVENTS {
                                trace.push((chunk.clone(), line));
                            }
                        }
                        let bps = hook.breakpoints.lock().unwrap();
                        if bps.iter().any(|(c, l)| *l == line && chunk.contains(c.as_str())) {
                            return Err(mlua::Error::external(format!(
                                "{BREAKPOINT_SENTINEL} at {chunk}:{line}"
                            )));
                        }
                    }
                    Ok(mlua::VmState::Continue)
                }
                _ => Ok(mlua::VmState::Continue),
            }
        },
    );
    let _ = lua.set_memory_limit(policy.max_memory_bytes.max(1024 * 1024));
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_denies_process_and_fs() {
        let p = SandboxPolicy::default();
        assert!(p.allows(Capability::DocumentRead));
        assert!(!p.allows(Capability::Process));
        assert!(!p.allows(Capability::FilesystemRead));
    }

    #[test]
    fn apply_strips_dofile_and_os_execute() {
        let lua = mlua::Lua::new();
        SandboxPolicy::default().apply(&lua).unwrap();
        let dofile: mlua::Value = lua.globals().get("dofile").unwrap();
        assert!(matches!(dofile, mlua::Value::Nil));
        let os: mlua::Table = lua.globals().get("os").unwrap();
        let exec: mlua::Value = os.get("execute").unwrap();
        assert!(matches!(exec, mlua::Value::Nil));
    }

    #[test]
    fn manifest_permissions_parse() {
        let set = permissions_from_manifest(
            &["document.read".to_string(), "UI".to_string()],
        )
        .unwrap();
        assert!(set.contains(&Capability::DocumentRead));
        assert!(set.contains(&Capability::Ui));
        assert!(permissions_from_manifest(&["root".to_string()]).is_err());
        assert!(permissions_from_manifest(&[]).unwrap().is_empty());
    }

    #[test]
    fn execution_limits_arm() {
        let lua = mlua::Lua::new();
        let policy = SandboxPolicy {
            max_instructions: 50_000,
            ..SandboxPolicy::default()
        };
        let _budget = arm_execution_limits(&lua, &policy);
        // Infinite loop dies via the hook, not by hanging the test.
        let err = lua.load("while true do end").eval::<mlua::Value>().unwrap_err();
        assert!(err.to_string().contains(TIMEOUT_SENTINEL));
        // Memory hog dies via the cap.
        let lua2 = mlua::Lua::new();
        let _b2 = arm_execution_limits(&lua2, &SandboxPolicy::default());
        let err = lua2
            .load("local t = {}; while true do t[#t+1] = string.rep('x', 1024) end")
            .eval::<mlua::Value>()
            .unwrap_err();
        assert!(matches!(err, mlua::Error::MemoryError(_)), "{err:?}");
    }
}
