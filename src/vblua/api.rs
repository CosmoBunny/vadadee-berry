//! `vblua.*` standard library (Phases 1 + 3 + 4).
//!
//! Platform-independent by construction: Lua sees `vblua.platform()` as an
//! informational string only. There is deliberately no
//! `vblua.android_* / vblua.ios_* / vblua.linux_*` surface —
//! platform behaviour stays behind Rust interfaces.

use std::sync::{Arc, Mutex};

use super::context::{DocumentCommand, DocumentSnapshot};
use super::error::VbluaError;
use super::sandbox::{Capability, SandboxPolicy};
use super::{API_VERSION, VBLUA_VERSION};

/// Mutable host side shared with every Lua callback.
pub struct ApiHost {
    pub snapshot: DocumentSnapshot,
    pub has_document: bool,
    pub commands: Vec<DocumentCommand>,
    pub policy: SandboxPolicy,
    pub console: Vec<String>,
}

impl ApiHost {
    fn push_command(&mut self, cmd: DocumentCommand) -> Result<bool, String> {
        if !self.policy.allows(Capability::DocumentWrite) {
            return Err("document.write capability is not granted".to_string());
        }
        if self.commands.len() >= self.policy.max_commands {
            return Err(format!(
                "too many staged commands (limit {})",
                self.policy.max_commands
            ));
        }
        self.commands.push(cmd);
        Ok(true)
    }
}

fn clamp01_len(s: &str) -> String {
    const MAX: usize = 4096;
    if s.len() > MAX {
        s[..MAX].to_string()
    } else {
        s.to_string()
    }
}

/// Register the `vblua` + `vb` tables plus the math helpers on `lua`.
pub fn register(lua: &mlua::Lua, host: Arc<Mutex<ApiHost>>) -> mlua::Result<()> {
    let globals = lua.globals();
    let vblua = lua.create_table()?;

    vblua.set("_VERSION", VBLUA_VERSION)?;
    vblua.set(
        "version",
        lua.create_function(|_, _: ()| Ok(VBLUA_VERSION.to_string()))?,
    )?;
    vblua.set(
        "api_version",
        lua.create_function(|_, _: ()| Ok(API_VERSION.to_string()))?,
    )?;
    vblua.set(
        "platform",
        lua.create_function(|_, _: ()| {
            // Informational only — scripts must use capability APIs, not branches.
            Ok(std::env::consts::OS.to_string())
        })?,
    )?;

    // --- logging: vblua.log / warn / error -> host console (never stdout) ---
    for (name, prefix) in [("log", ""), ("warn", "WARN: "), ("error", "ERROR: ")] {
        let h = host.clone();
        let p = prefix.to_string();
        vblua.set(
            name,
            lua.create_function(move |_, msg: mlua::Value| {
                let text = match msg {
                    mlua::Value::String(s) => s.to_str()?.to_string(),
                    mlua::Value::Integer(i) => i.to_string(),
                    mlua::Value::Number(n) => n.to_string(),
                    mlua::Value::Boolean(b) => b.to_string(),
                    mlua::Value::Nil => "nil".into(),
                    other => format!("{other:?}"),
                };
                let mut h = h.lock().unwrap();
                h.console.push(format!("{p}{}", clamp01_len(&text)));
                if h.console.len() > 512 {
                    let drain = h.console.len() - 512;
                    h.console.drain(..drain);
                }
                Ok(())
            })?,
        )?;
    }

    // --- math (Rust-backed, no reinvented numerics) ---
    let math = lua.create_table()?;
    math.set(
        "clamp",
        lua.create_function(|_, (v, lo, hi): (f64, f64, f64)| Ok(v.clamp(lo.min(hi), lo.max(hi))))?,
    )?;
    math.set(
        "lerp",
        lua.create_function(|_, (a, b, t): (f64, f64, f64)| Ok(a + (b - a) * t))?,
    )?;
    math.set(
        "smoothstep",
        lua.create_function(|_, (e0, e1, x): (f64, f64, f64)| {
            let t = ((x - e0) / (e1 - e0).max(f64::EPSILON)).clamp(0.0, 1.0);
            Ok(t * t * (3.0 - 2.0 * t))
        })?,
    )?;
    for (name, n) in [("vec2", 2), ("vec3", 3), ("vec4", 4)] {
        math.set(
            name,
            lua.create_function(move |lua, args: mlua::MultiValue| {
                let vals: Vec<f64> = args
                    .into_iter()
                    .map(|v| match v {
                        mlua::Value::Integer(i) => Ok(i as f64),
                        mlua::Value::Number(x) => Ok(x),
                        _ => Err(mlua::Error::BadArgument {
                            to: Some(name.to_string()),
                            pos: 1,
                            name: Some("number".to_string()),
                            cause: std::sync::Arc::new(mlua::Error::external(
                                "vblua.math: expected numbers",
                            )),
                        }),
                    })
                    .collect::<mlua::Result<_>>()?;
                if vals.len() != n {
                    return Err(mlua::Error::BadArgument {
                        to: Some(name.to_string()),
                        pos: vals.len() + 1,
                        name: Some(format!("expected {n} numbers")),
                        cause: std::sync::Arc::new(mlua::Error::external(format!(
                            "vblua.math.{name}: expected {n} numbers, got {}",
                            vals.len()
                        ))),
                    });
                }
                let t = lua.create_table()?;
                for (i, v) in vals.iter().enumerate() {
                    t.set(i + 1, *v)?;
                }
                Ok(t)
            })?,
        )?;
    }
    vblua.set("math", math)?;

    // --- document API (Phase 4): read from snapshot, write via command queue ---
    let document = lua.create_table()?;
    {
        let h = host.clone();
        document.set(
            "current",
            lua.create_function(move |lua, _: ()| {
                let h = h.lock().unwrap();
                if !h.has_document {
                    return Ok(mlua::Value::Nil);
                }
                Ok(mlua::Value::Table(h.snapshot.clone().into_lua_table(lua)?))
            })?,
        )?;
    }
    {
        let h = host.clone();
        document.set(
            "rename",
            lua.create_function(move |_, title: String| {
                h.lock()
                    .unwrap()
                    .push_command(DocumentCommand::Rename { title })
                    .map_err(mlua::Error::external)
            })?,
        )?;
    }
    {
        let h = host.clone();
        document.set(
            "resize",
            lua.create_function(move |_, (w, hgt): (f64, f64)| {
                h.lock()
                    .unwrap()
                    .push_command(DocumentCommand::Resize {
                        width: w,
                        height: hgt,
                    })
                    .map_err(mlua::Error::external)
            })?,
        )?;
    }
    {
        let h = host.clone();
        document.set(
            "set_layer_visible",
            lua.create_function(move |_, (id, visible): (String, bool)| {
                h.lock()
                    .unwrap()
                    .push_command(DocumentCommand::SetLayerVisible {
                        layer_id: id,
                        visible,
                    })
                    .map_err(mlua::Error::external)
            })?,
        )?;
    }
    // File I/O is intentionally unavailable to scripts: it must go through the
    // platform file abstraction (Phase 12). Loud stub, not silent nil.
    for name in ["open", "save", "close", "create"] {
        document.set(
            name,
            lua.create_function(move |_, _: ()| {
                Err::<bool, _>(mlua::Error::external(format!(
                    "document.{name}() is not scriptable yet — file access goes through the Vadadee Berry picker (Phase 12)"
                )))
            })?,
        )?;
    }
    vblua.set("document", document)?;

    globals.set("vblua", vblua.clone())?;
    // Short alias (mirrors the `traya`/`tlua` convention in the sibling project).
    globals.set("vb", vblua)?;
    Ok(())
}
