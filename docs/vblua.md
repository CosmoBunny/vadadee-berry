# VBLua — Vadadee Berry's Programmable Platform

Status: **Phases 1–4 implemented** (foundation, lifecycle, stdlib, document API).

## Architecture (non-negotiable)

```text
Lua → VBLua API → Rust abstraction → platform implementation
```

Lua never sees pointers, GPU resources, filesystem paths, or platform APIs.
Mutations cross as a **batched command queue** applied by Rust after the
script returns — one script run = one undo transaction.

## Runtime lifecycle (Phase 2)

```rust
let mut rt = VbRuntime::new(SandboxPolicy::default())?;
rt.execute("hello.lua", r#"vblua.log("hi"); return 1"#)?;
rt.pause(); rt.resume();
rt.reset()?;   // fresh interpreter
rt.shutdown(); // permanent
```

Every failure is a structured `VbluaError` (`Syntax/Runtime/Api/Permission/
Timeout/Resource/Internal`) — a script can never crash the host.

## Standard library (Phases 1+3)

```lua
vblua.version()      -- "0.1.0"
vblua.api_version()  -- "1" (manifests pin this)
vblua.platform()     -- informational OS string only; use capabilities, not branches
vblua.log / warn / error
vblua.math.clamp(v, lo, hi)
vblua.math.lerp(a, b, t)
vblua.math.smoothstep(e0, e1, x)
vblua.math.vec2(x, y) / vec3 / vec4
```

## Document API (Phase 4)

```lua
local d = vblua.document.current() -- nil when no document
print(d.name, d.width, d.height, d.layer_count)
vblua.document.rename("Title")        -- needs document.write
vblua.document.resize(1920, 1080)     -- clamped 1..16384
vblua.document.set_layer_visible(id, true)
-- document.open/save/close/create: NOT scriptable (Phase 12 picker only)
```

## Sandbox (Phase 17 groundwork)

Default policy: `document.read` only. `dofile/loadfile/require`,
`os.execute/exit`, `io.*`, `debug.*`, `package.loadlib` are stripped.
`print` routes to the script console (capped at 512 lines).

## Value boundary (Phase 6 groundwork)

`VbValue <-> mlua::Value`: nil/bool/int/number/string, `Vec2/3/4`, `Color`,
`NodeRef`/`AssetRef` (UUID strings, never pointers), `Map`/`List`.
Functions, threads, and userdata are rejected at the boundary.

## Roadmap

Next: node graph API (5–6), animation (7), assets + file picker (11–12),
UI scripting (13), sandbox manifests + addons (17–19), Android/iOS
validation on real APK/IPA (22–23), perf + error UX (24–25).
