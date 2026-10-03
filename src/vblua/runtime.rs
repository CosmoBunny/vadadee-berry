//! VBLua runtime lifecycle (Phase 2):
//! `Create -> Initialize -> Load -> Execute -> Pause/Resume -> Reset -> Shutdown`.
//!
//! The app stays fully functional with no script: the runtime is created
//! lazily, and every failure surfaces as [`VbluaError`] — never a panic, never
//! a host crash. Mutations apply as one batch after the script returns, so one
//! script run can become one undo transaction (Phase 16).

use std::sync::{Arc, Mutex};

use super::api::{register, ApiHost};
use super::context::{DocumentCommand, DocumentSnapshot};
use super::error::{VbluaError, from_mlua};
use super::sandbox::SandboxPolicy;
use crate::document::Document;

/// Lifecycle state. `Paused` blocks `execute`; `Shutdown` drops the Lua state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lifecycle {
    #[default]
    Created,
    Ready,
    Paused,
    Shutdown,
}

/// Embedded Lua runtime. Owns the interpreter; borrows the [`Document`] only
/// for the duration of `execute_with_document` (snapshot in, commands out).
pub struct VbRuntime {
    lua: Option<mlua::Lua>,
    host: Arc<Mutex<ApiHost>>,
    policy: SandboxPolicy,
    state: Lifecycle,
    /// Script console (`vblua.log` + Lua `print`), drained by the UI.
    pub console: Vec<String>,
}

impl VbRuntime {
    /// Create + initialize (fresh interpreter, API registered, sandbox armed).
    pub fn new(policy: SandboxPolicy) -> Result<Self, VbluaError> {
        let host = Arc::new(Mutex::new(ApiHost {
            snapshot: DocumentSnapshot::default(),
            has_document: false,
            commands: Vec::new(),
            policy: policy.clone(),
            console: Vec::new(),
        }));
        let mut rt = Self {
            lua: None,
            host,
            policy,
            state: Lifecycle::Created,
            console: vec![format!(
                "VBLua {} (Lua 5.4) ready.",
                super::VBLUA_VERSION
            )],
        };
        rt.reset()?;
        Ok(rt)
    }

    pub fn state(&self) -> Lifecycle {
        self.state
    }

    pub fn pause(&mut self) {
        if self.state == Lifecycle::Ready {
            self.state = Lifecycle::Paused;
        }
    }

    pub fn resume(&mut self) {
        if self.state == Lifecycle::Paused {
            self.state = Lifecycle::Ready;
        }
    }

    /// Drop the interpreter and build a fresh one (clears globals + console).
    pub fn reset(&mut self) -> Result<(), VbluaError> {
        let lua = mlua::Lua::new();
        self.policy
            .apply(&lua)
            .map_err(|e| VbluaError::Internal(format!("sandbox setup failed: {e}")))?;
        register(&lua, self.host.clone())
            .map_err(|e| VbluaError::Internal(format!("API registration failed: {e}")))?;
        self.install_print_hook(&lua);
        self.lua = Some(lua);
        {
            let mut h = self.host.lock().unwrap();
            h.commands.clear();
            h.console.clear();
        }
        self.state = Lifecycle::Ready;
        Ok(())
    }

    /// Permanently release the interpreter. Any later `execute` fails cleanly.
    pub fn shutdown(&mut self) {
        self.lua = None;
        self.state = Lifecycle::Shutdown;
    }

    /// `print(...)` routes to the script console (capped), like `vblua.log`.
    fn install_print_hook(&self, lua: &mlua::Lua) {
        let host = self.host.clone();
        let print_fn = lua.create_function(move |_, args: mlua::MultiValue| {
            let mut parts = Vec::new();
            for a in args {
                parts.push(match a {
                    mlua::Value::String(s) => {
                        s.to_str().map(|s| s.to_string()).unwrap_or_default()
                    }
                    mlua::Value::Integer(i) => i.to_string(),
                    mlua::Value::Number(n) => n.to_string(),
                    mlua::Value::Boolean(b) => b.to_string(),
                    mlua::Value::Nil => "nil".into(),
                    other => format!("{other:?}"),
                });
            }
            let line = parts.join("\t");
            let mut h = host.lock().unwrap();
            h.console.push(line.chars().take(4096).collect());
            if h.console.len() > 512 {
                let drain = h.console.len() - 512;
                h.console.drain(..drain);
            }
            Ok(())
        });
        if let Ok(f) = print_fn {
            let _ = lua.globals().set("print", f);
        }
    }

    /// Move host console lines into the runtime console.
    fn flush_console(&mut self) {
        let lines = {
            let mut h = self.host.lock().unwrap();
            std::mem::take(&mut h.console)
        };
        self.console.extend(lines);
        if self.console.len() > 1024 {
            let drain = self.console.len() - 1024;
            self.console.drain(..drain);
        }
    }

    /// Execute a script with no document (pure `vblua.*` + math).
    pub fn execute(&mut self, script: &str, code: &str) -> Result<String, VbluaError> {
        self.execute_with_document(script, code, None)
    }

    /// Execute with a document snapshot; queued commands apply to `doc` after.
    /// Returns the script's converted result (`"nil"` for no value).
    pub fn execute_with_document(
        &mut self,
        script: &str,
        code: &str,
        doc: Option<&mut Document>,
    ) -> Result<String, VbluaError> {
        if self.state == Lifecycle::Shutdown {
            return Err(VbluaError::Internal("runtime is shut down".into()));
        }
        if self.state == Lifecycle::Paused {
            return Err(VbluaError::Permission {
                capability: "execute".into(),
                message: "runtime is paused".into(),
            });
        }
        if code.len() > 1_000_000 {
            return Err(VbluaError::Resource {
                resource: "script".into(),
                message: "script exceeds 1 MiB".into(),
            });
        }

        // Snapshot in, queue cleared.
        {
            let mut h = self.host.lock().unwrap();
            h.commands.clear();
            match doc.as_deref() {
                Some(d) => {
                    h.snapshot = DocumentSnapshot::capture(d);
                    h.has_document = true;
                }
                None => {
                    h.snapshot = DocumentSnapshot::default();
                    h.has_document = false;
                }
            }
        }

        let Some(lua) = &self.lua else {
            return Err(VbluaError::Internal("interpreter missing".into()));
        };
        // `print` may have been clobbered by a previous script — restore it.
        self.install_print_hook(lua);

        let result = match lua.load(code).set_name(script).eval::<mlua::Value>() {
            Ok(v) => {
                self.flush_console();
                Ok(match v {
                    mlua::Value::Nil => "nil".into(),
                    mlua::Value::Integer(i) => i.to_string(),
                    mlua::Value::Number(n) => n.to_string(),
                    mlua::Value::Boolean(b) => b.to_string(),
                    mlua::Value::String(s) => {
                        s.to_str().map(|s| s.to_string()).unwrap_or_default()
                    }
                    other => format!("{other:?}"),
                })
            }
            Err(e) => {
                self.flush_console();
                Err(from_mlua(script, e))
            }
        };

        // Batch out: apply queued commands even when the script errored
        // (partial work is kept, matching the traya convention).
        let commands: Vec<DocumentCommand> = {
            let mut h = self.host.lock().unwrap();
            std::mem::take(&mut h.commands)
        };
        if let Some(d) = doc {
            Self::apply_commands(d, &commands);
        }
        result
    }

    /// Apply a staged batch. Returns the number of commands that changed state.
    /// The caller wraps this in one undo transaction (Phase 16).
    pub fn apply_commands(doc: &mut Document, commands: &[DocumentCommand]) -> usize {
        commands.iter().filter(|c| c.apply_to(doc)).count()
    }

    /// Drain staged commands without a document (testing / headless use).
    #[cfg(test)]
    fn take_commands(&self) -> Vec<DocumentCommand> {
        std::mem::take(&mut self.host.lock().unwrap().commands)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_doc() -> Document {
        Document {
            title: "t".into(),
            width: 100.0,
            height: 100.0,
            layers: vec![],
            active_layer_index: 0,
            defs: Default::default(),
            path_effects: Default::default(),
            tiling_effects: Default::default(),
            circular_effects: Default::default(),
            clip_masks: Default::default(),
            boolean_effects: Default::default(),
            page_color: [1.0, 1.0, 1.0, 1.0],
            page_unit: Default::default(),
        }
    }

    #[test]
    fn hello_script_runs() {
        let mut rt = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
        let out = rt
            .execute("hello.lua", r#"vblua.log("Hello"); return vblua.version()"#)
            .unwrap();
        assert_eq!(out, super::super::VBLUA_VERSION);
        assert!(rt.console.iter().any(|l| l.contains("Hello")));
    }

    #[test]
    fn syntax_error_is_structured() {
        let mut rt = VbRuntime::new(SandboxPolicy::default()).unwrap();
        let err = rt.execute("bad.lua", "local x = ").unwrap_err();
        assert!(matches!(err, VbluaError::Syntax { .. }), "{err:?}");
    }

    #[test]
    fn runtime_error_does_not_crash_host() {
        let mut rt = VbRuntime::new(SandboxPolicy::default()).unwrap();
        let err = rt.execute("boom.lua", r#"nil_fn()"#).unwrap_err();
        assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
        // Host still works afterwards.
        assert_eq!(rt.execute("ok.lua", "return 1").unwrap(), "1");
    }

    #[test]
    fn document_read_and_queued_write_batch() {
        let mut rt = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
        let mut doc = test_doc();
        let out = rt
            .execute_with_document(
                "doc.lua",
                r#"
                local d = vblua.document.current()
                vblua.document.rename("New")
                vblua.document.resize(200, 300)
                return d.title .. ":" .. d.width
                "#,
                Some(&mut doc),
            )
            .unwrap();
        assert!(out == "t:100" || out == "t:100.0", "got {out}");
        assert_eq!(doc.title, "New");
        assert_eq!((doc.width, doc.height), (200.0, 300.0));
        // Read-only policy denies writes with a structured error; doc unchanged.
        let mut ro = VbRuntime::new(SandboxPolicy::default()).unwrap();
        let mut doc2 = test_doc();
        let err = ro
            .execute_with_document(
                "ro.lua",
                r#"vblua.document.rename("X"); return vblua.document.current().title"#,
                Some(&mut doc2),
            )
            .unwrap_err();
        assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
        assert_eq!(doc2.title, "t");
    }

    #[test]
    fn pause_resume_reset_shutdown() {
        let mut rt = VbRuntime::new(SandboxPolicy::default()).unwrap();
        rt.pause();
        assert!(rt.execute("x.lua", "return 1").is_err());
        rt.resume();
        assert_eq!(rt.execute("x.lua", "return 1").unwrap(), "1");
        rt.reset().unwrap();
        assert_eq!(rt.execute("x.lua", "return 1").unwrap(), "1");
        rt.shutdown();
        assert!(rt.execute("x.lua", "return 1").is_err());
    }

    #[test]
    fn sandbox_strips_io() {
        let mut rt = VbRuntime::new(SandboxPolicy::default()).unwrap();
        let out = rt.execute("io.lua", "return io.open").unwrap();
        assert_eq!(out, "nil");
    }

    #[test]
    fn math_helpers_work() {
        let mut rt = VbRuntime::new(SandboxPolicy::default()).unwrap();
        assert_eq!(
            rt.execute("m.lua", "return vblua.math.clamp(5, 0, 3)").unwrap(),
            "3"
        );
        assert_eq!(
            rt.execute("m.lua", "return vblua.math.lerp(0, 10, 0.5)").unwrap(),
            "5"
        );
    }
}
