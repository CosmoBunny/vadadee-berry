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

## Node graph API (Phase 5)

```lua
local a = vblua.graph.create("Blur", { x = 10, y = 20 })
local b = vblua.graph.create("Blur", { x = 200, y = 20 })
vblua.graph.connect(a, "out", b, "in")   -- validated now (ports/types/cycles)
vblua.graph.rename(a, "First")
vblua.graph.get(a)          -- {id, name, kind, layer, links}
vblua.graph.find("First")   -- [ids] (name substring)
vblua.graph.layers()        -- [{id, name, nodes, links}]
vblua.graph.nodes()         -- target layer's nodes
vblua.graph.disconnect(b, "in")
vblua.graph.duplicate(a)    -- -> new id
vblua.graph.remove(a)
```

IDs are Uuid strings, usable in the same run (pre-allocated). File-backed
kinds (`Image`, `Video`, `ObjectFromApp`, `Param*`) are denied — they need
the Asset API (Phase 11). Wires validate ports/direction/types/cycles at
call time, mirroring the host `try_add_link`.

## Node parameters (Phase 6)

```lua
vblua.graph.set_param(id, "value", 42)   -- Value nodevblua.graph.set_param(id, "expr", "x*2") -- ExprX/Xy/Xyz (capped at 512 chars)
vblua.graph.set_param(id, "gain", 1.5)   -- MouseEncoder/Visualizer (clamped)
vblua.graph.get_param(id, "value")        -- live value, incl. this run's sets
vblua.graph.params(id)                    -- {value = "number"} (discovery)
```

Values cross the `VbValue` layer (`Int/Num -> number`, `Str -> string`);
bools/nil/tables are rejected with a message until a field takes them.
Effect amounts (blur radius, …) arrive via input wires, not fields — unit
variants expose no keys. Parameter-tab (`Param*`) editing comes with the
Asset API.

## Animation API (Phase 7)

```lua
vblua.animation.set_keyframe(id, "rotation", 0, 0)
vblua.animation.set_keyframe(id, "rotation", 60, 360, "bezier")
vblua.animation.sample(id, "rotation", 15)      -- 90.0 (keyframes only)
vblua.animation.keyframes(id, "rotation")       -- [{frame, value, interp}]
vblua.animation.remove_keyframe(id, "rotation", 60)
vblua.animation.tracks()   -- label list (+ geom_N, param:* patterns)
vblua.animation.max_frame() -- content span at fps=60
```

Targets are canvas-node or layer ids. Interpolation is `linear`/`bezier`
(`smooth` aliases bezier; `step`/`cubic`/`custom` fail — the host has no
such modes). Stack-function spans are not scriptable yet. Timeline edits
land in one `PatchTimeline` undo entry via the console.

## Addon system (Phase 18)

```text
my-addon/
├── manifest.lua   -- return { id, name, version, vblua = "1", permissions?, deps? }
├── addon.lua      -- runs on enable
└── README.md
```
```lua
vblua.addons.refresh()          -- rescan <data-dir>/addons
vblua.addons.enable("example.myaddon")  -- deps checked, perms granted, code runs
vblua.addons.list() / info(id) / disable(id) / uninstall(id)
```

Manifests parse in a throwaway sandbox; `vblua` is major-gated; deps must
be enabled first (disabling cascades). Explicit `enable` = consent:
manifest permissions union into the policy (logged). Failures keep the
addon disabled. See `examples/vblua/my-addon/`. Packages (`export_bundle`
/ `install_bundle`, Stored zips, traversal/size-capped) are Rust-side local
sharing — no marketplace.

## Kinematics API (Phase 8)

```lua
local arm = vblua.kinematics.chain({ 100, 80, 60 })
local angles = arm.solve({ x = 150, y = 40 })  -- CCD IK, absolute radians
local joints = arm.fk(angles)                   -- [{x, y}...], base is [1]
local s = vblua.kinematics.ik2(100, 80, {x=0,y=0}, {x=150,y=40}) -- analytic
vblua.kinematics.fk({100, 80}, { a1, a2 })      -- raw FK
vblua.kinematics.ik(lengths, target, { base=.., initial=.., iters=.. })
vblua.kinematics.damp(c, t, lambda, dt)         -- framerate-independent ease
vblua.kinematics.spring(pos, vel, target, k, c, dt) -- one Euler step
vblua.kinematics.look_at(from, to)              -- radians
vblua.kinematics.orbit(center, radius, angle)   -- {x, y}
```

Pure functions: Rust computes, Lua describes. Solvers return angles —
writing poses stays with `animation.set_keyframe` (same undo batch), so no
new mutation channel exists. Chains cap at 16 segments / 256 iterations.

## Shader API (Phase 9)

```lua
vblua.shader.validate(src)          -- pure static check -> true or error
local p = vblua.shader.create(nil, "My Effect") -- disabled template pass
vblua.shader.set_source(p, src)     -- validated now (GLSL/compute rejected)
vblua.shader.set_uniforms(p, { 0.0, 0.5 })     -- <=64 finite floats
vblua.shader.set_enabled(p, true)
vblua.shader.passes()               -- [{id, name, enabled, uniforms, ...}]
vblua.shader.rename(p, "Glow") / vblua.shader.remove(p)
```

New passes start disabled (present but inert). Sources cap at 64 KiB;
GPU compile remains host-side on enable (failures surface in the pass
`compile_error`, shown in UI). No file paths cross into Lua.

## Video API (Phase 10)

```lua
vblua.video.clips()              -- [{id, name, kind, start, length, offset, row, duration}]
vblua.video.get(id)
vblua.video.move(id, 12.5)
vblua.video.trim(id, { offset = 2.0, length = 8.0 })
local right = vblua.video.split(id, 14.0)  -- validated span, returns new id
vblua.video.set_row(id, 1) / rename(id, "Intro") / remove(id)
```

Existing media only: `media_path` never crosses (scripts see `kind` +
cached `duration`). `add_clip(path)` is denied — new media needs the Asset
API (Phase 11) + picker (Phase 12). Speed/reverse/transitions don't exist
on the host clip yet, so they're omitted, not faked. Clip edits ride the
`PatchDocument` undo via the console.

## Asset API (Phase 11)

```lua
vblua.assets.list()          -- [{ref, kind, name, width?, height?, bytes?, duration?}]
vblua.assets.list("image")   -- kind filter
vblua.assets.info(ref)       -- detail table | nil
vblua.assets.kinds()         -- ["audio", "image", ...] present
```

Handles are opaque (`node:<uuid>` / `clip:<uuid>`); `media_path` and raw
bytes never cross (asserted in tests). Read-only — no undo, no capability
beyond document read. `import` is denied: new bytes enter only through the
platform picker (Phase 12).

## Sandbox enforcement (Phase 17)

| Capability | Gates | Default |
|---|---|---|
| `document.read` | snapshots (doc/graph/anim/assets) | ✅ granted |
| `document.write` | all command queues | ❌ denied |
| `asset.read` | (reserved: asset mutation) | ❌ denied |
| `ui` | `vblua.ui.*` panels | ❌ denied |
| `clipboard` | (no API surface — enforced by absence) | ❌ denied |
| `network` | (no API surface — enforced by absence) | ❌ denied |
| `filesystem.read` | `file.pick` | ❌ denied |
| `filesystem.write` | (no API surface — enforced by absence) | ❌ denied |
| `process` | (no API surface — enforced by absence) | ❌ denied |

Plus: instruction budget per run (`Timeout`, default 2M), 64 MiB Lua heap
cap (`MemoryError`), 1 MiB script cap, 512-line console cap, 4096-command
cap. `dofile/loadfile/require`, `os.execute/exit`, `io.*`, `debug.*`,
`package.loadlib` are stripped. Manifests declare permissions by these
exact names (`vblua.permissions()` lists current grants).

## Mobile posture (Phase 21) + Android validation (Phase 22)

```lua
if vblua.has("file_picker") then
  local id = vblua.file.pick({ filters = { "png" } })
end
vblua.has("document.write")  -- policy mirror
vblua.has("addon_storage")   -- persistent data dir present
```

Unknown names report absent (never claim what isn't there). Audit result:
`src/vblua` uses no `rfd/arboard/tokio/process/net`; the only `std::fs`
is the addon manager on the app-private data dir (+ tests) — no desktop
paths, no system locations. `aarch64-linux-android --no-default-features`
`cargo check` passes locally (NDK 29, incl. vendored Lua C); iOS needs CI
(Xcode SDK, no local target). APK-level runtime validation stays on CI
(dispatch/tag) per project policy.

## Security review (Phase 34)

Reviewed attack surface, all with regression tests:
- Infinite loops → instruction budget (`Timeout`); heap bombs → 64 MiB cap.
- Deep/wide Lua tables → boundary caps (depth 64, breadth 4096/table).
- Unbounded arg loops → per-call caps (params 64, filters 32, links 128,
  specs 512, keys 2048, panels 32×128, subscribers 16/event).
- Dangerous stdlib asserted stripped (`dofile/loadfile/require/io/os/debug`).
- Paths/bytes never cross (inventory + picker-bytes-only, asserted).
- Events are one-level (no infinite recursion); transactions roll back.
- Open risks (accepted): host-trusted addon dirs (local user = trust root);
  GPU compile errors surface post-enable (validator is static-only).

## Performance (Phase 24) + error UX (Phase 25)

`cargo run --no-default-features --bin vblua_bench` (local dev machine):

```text
runtime startup:     0.64 ms    hello execute:       0.06 ms
100-node batch:      2.26 ms    101-keyframe batch:  0.70 ms
CCD IK solve:        0.10 ms    template define+spawn: 0.46 ms
manifest parse:      0.32 ms
```

No hotspots at these scales — optimize on deltas, not guesses. Errors
render as `VBLua Error [Kind] / detail / Suggestion:` (`VbluaError::render`,
pushed verbatim by the console); interpreter line info passes through
untouched.

## Versioning + maturity (Phase 30/35) + test suite (Phase 33)

- `vblua.version()` (impl), `vblua.api_version()` (`"1"`, manifests pin
  majors), `vblua.maturity()` (`experimental` — no stability promised yet).
- `tests/vblua_api.rs`: black-box integration (pipeline, rollback,
  read-only policy, kinematics→keyframes) alongside 65 unit tests.

## Events + hot-reload (Phase 27–28)

```lua
vblua.events.on("node-created", function(ids) ... end)
vblua.events.on("error", function(msg) ... end)
vblua.events.list()  -- after_run, node-created, node-deleted, addon-enabled, error
vblua.addons.reload("example.glow")  -- re-read + re-run addon.lua
```

Events fire from the outer batch only — callbacks may queue work (second
drain, same undo) but never fire further events: one level, no loops.
App-level events (open/export/selection) need app dispatch points and are
not faked.

`document-open` fires after a project loads; `before-export` fires before
SVG export (subscriber edits land in the file). Both go through the same
batch + undo path. `selection-change` remains future (event-loop state).

## GPU kernel runtime, milestone 1 (image analysis + kernel validation)

```lua
local img = vblua.image.solid(64, 64, { 1, 0, 0, 1 })
vblua.image.average_brightness(img)   -- 0.2126 (GPU reduction or CPU oracle)
vblua.image.region_average(img, 0.25, 0.25, 0.5, 0.5)  -- normalized rect
vblua.image.luminance(img)            -- grayscale handle (feeds other ops)
vblua.image.grayscale/brightness/contrast(img, v) -> handle
vblua.image.average_color/histogram/sample(img, u, v)
vblua.kernel.create({ name, language = "wgsl", source, parameters })
```

Rec.709 on stored bytes, 0..1. GPU path (llvmpipe-verified): luminance
compute → tree reduction → finalize → ONE 256-float readback; CPU fallback
is bit-identical. Custom WGSL validates now (compute entry, brackets, param names).
`image.apply(frame, kernel, {params})` executes on GPU (llvmpipe-verified,
incl. the color_balance reference) with cached pipelines (hash key) and
override merging; needs a device (honest error without one). Chains compose
(`a→b→c` verified). Handles capped (64 images, 32 kernels); coordinates
normalized, `0,0` top-left.

GPU bench (llvmpipe software GL — worst case; hardware will be far faster):

```text
512px brightness: ~35 ms    256px kernel apply: ~14 ms
```

Failure taxonomy (spec §23 — all structured, never panics):
`LuaSyntaxError/RuntimeError` → `Syntax`/`Runtime`; kernel validation,
bad params/formats → `Api`; device loss/absence → `Runtime` with
`GPU ...` text; budget/memory → `Timeout`/`Resource`. Driver validation
panics are contained behind `catch_unwind`.

## Professional timeline + media nodes (milestone 1)

Video → Speed → ZoomVideo → Output works end to end: `TimeMap` (affine
`offset = m·t + b`) composes Speed/Reverse uniformly, downstream time nodes
remap `video_time_sec` from the player window (previously a silent no-op),
negative Speed is a clear error pointing at Reverse, split is undoable.
`TimeOffset` shifts source time (blank out-of-window), `FreezeFrame` holds
one timestamp across all output times (decoder seeks once per timestamp).
`Transform` merges placement/size/rotation in one node; `Crop` is a
normalized rect applied as paint-UV intersection (preview) and baked crop
(export) — parity by construction; `FlipHorizontal/Vertical` set mirror bits.
Clips gained `muted` (preview/export skip) and `locked` (refuses all edits)
plus undoable duplication sharing the source file. Document-level timeline
markers (`add/move/rename/remove`, kept sorted) double as snap targets:
clip drags (move + both trims) snap to clip edges and markers within 0.2s.
`TimeRemap` adds piecewise-linear time curves: points live on the node
(`curve` param `{{t,s},...}`), evaluation pushes a warp onto the `TimeMap`
stack — downstream Speed/Reverse/Offset stay exact, chained remaps nest,
warped audio reports neutral rate, out-of-window still blanks. Empty curve
is an identity passthrough.
VBLua: `reverse`/`zoomvideo`/`zoomimage` kinds. Both resolve entry points
share one implementation (no preview/export drift by construction).

## Tracing debugger (Phase 26)
```lua
vblua.debug.trace(function() ... end)  -- run with line tracing
vblua.debug.trace_events()             -- [{chunk, line}...] (capped 4096)
vblua.debug.breakpoint("script.lua", 42)  -- abort with chunk:line info
vblua.debug.clear_breakpoints()
```

One unified hook serves budget + tracer + breakpoints (Lua allows one).
No locals/watch by design — `debug.*` stays stripped. Stepping (pause /
resume execution) needs coroutines and is future work.

## Editor adapter (vblua-lsp core)

`src/vblua/lsp.rs` + `src/bin/vblua_lsp.rs`: stdio JSON-RPC server
(Content-Length framing, stderr-only logging) with full text sync,
syntax diagnostics (compile-only, never runs code; chunk named after the
document so host paths never leak), prefix completion, and hover — all
backed by one shared `API_ENTRIES` table (a test enforces every CLASS
entry parses as a real graph kind). Definition/references/rename/format
are NOT advertised. Run: `./target/debug/vblua_lsp` (speaks LSP on
stdin/stdout).

## UI scripting (Phase 13)

```lua
vblua.ui.panel("tools", "My Tool")
vblua.ui.button("tools", "Add Blur", function()
  vblua.graph.create("Blur", { x = 40, y = 40 })
end)
vblua.ui.slider("tools", "Radius", 0, 100, 12, function(v) ... end)
vblua.ui.checkbox("tools", "Enabled", true, function(on) ... end)
vblua.ui.text("tools", "hint") / vblua.ui.clear("tools") / drop_panel / panels()
```

Lua declares; the host renders panels in a floating window (32 panels,
128 widgets each). Callbacks run post-frame through the registry —
errors become console lines — and their document edits land in the
usual undo entries. Needs the `ui` capability (default policy denies).

## File picker abstraction (Phase 12)

```lua
if vblua.file.status().picker then
  local id = vblua.file.pick({ filters = { "png", "jpg" } }) -- -> node id
end
```

`pick()` calls the host `PickerHook` (desktop: rfd; Android: SAF;
iOS: document picker — mobile unwired, `status().picker == false`).
Bytes go hook → runtime (decode, 8 MiB / 4096px caps) → Image node on
the active (or first) image layer. Paths never touch Lua. Needs
`filesystem.read`. Imported nodes land in one `InsertNodesApplied`
undo entry via the console.

## Scripted node types (Phase 14)

```lua
vblua.node.define("SoftGlow", {
  nodes = { { kind = "Blur" }, { kind = "Brightness", name = "Lift" } },
  links = { { from = 1, from_port = "out", to = 2, to_port = "in" } },
})
local ids = vblua.node.spawn("SoftGlow", { x = 100, y = 100 })
```

Recipes validate at `define` (kinds, params, ports, cycles); the engine
owns the parsed template. `spawn` emits ordinary host nodes through the
standard graph queue — no Lua-owned lifetimes, no per-frame script
closures. 64 templates, 32 nodes each.

## Editor automation (Phase 16)

```lua
vblua.editor.transaction(function()
  vblua.graph.create("Blur")
  error("boom")   -- whole batch discarded, nothing applies
end)
vblua.editor.nodes()                    -- [{id, name, layer}]
vblua.editor.rename_node(id, "Hero")
local copy = vblua.editor.duplicate_node(id)
vblua.editor.delete_node(copy)
```

Normal runs keep partial work on error; `transaction()` is all-or-nothing.
Node ops validate against the canvas inventory and ride precise undo
entries (`InsertNodesApplied` / exact `RemoveNodes`). App-owned state
(selection, clipboard, playback, export) is intentionally unexposed.

## Procedural systems (Phase 15)

```lua
local pts = vblua.batch.grid({ x = 0, y = 0 }, 10, 60, 60, 100)
local specs = {}
for i, p in ipairs(pts) do specs[i] = { kind = "Value", x = p.x, y = p.y } end
local ids = vblua.batch.create_nodes(specs)   -- ONE call, 100 nodes
vblua.batch.circle(center, radius, count[, phase])
vblua.batch.keyframes(id, "rotation", { { 0, 0 }, { 60, 360 } })
```

Bulk ops share single-op validation and land in the same undo batches
(512 specs / 2048 keyframes per call, policy-capped). Layout helpers are
pure math, like kinematics.

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
