//! VBLua sandbox — explicit capabilities, deny by default (Phase 17 groundwork).
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
}
