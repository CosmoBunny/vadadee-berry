# VBLua API Reference (`experimental`)

Every public function: signature, capability, errors. Anything not listed
here does not exist — scripts must not rely on undocumented behavior.

Conventions: ids are UUID strings; `nil` means absent; errors are
`VbluaError::{Syntax,Runtime,Api,Permission,Timeout,Resource,Internal}`.

## Core — no capability needed

```lua
vblua.version()      -- "0.1.0" (impl)
vblua.api_version()  -- "1" (manifest major)
vblua.maturity()     -- "experimental"
vblua.platform()     -- OS string, informational only (never branch on it)
vblua.log(x) / warn(x) / error(x)  -- console (capped 512 lines)
vblua.has(name)      -- capability/feature probe; unknown → false
vblua.permissions()  -- granted capability names
```

## `vblua.math` — pure

`clamp(v,lo,hi)`, `lerp(a,b,t)`, `smoothstep(e0,e1,x)`,
`vec2(x,y)` / `vec3(x,y,z)` / `vec4(x,y,z,w)` → tables.
Errors: non-numbers, wrong arity.

## `vblua.document` — read: none; write: `document.write`

```lua
d = vblua.document.current()       -- nil without project; {name,title,width,height,layers,layer_count}
vblua.document.rename(title)       -- 1..256 chars
vblua.document.resize(w, h)        -- clamped 1..16384
vblua.document.set_layer_visible(id, bool)
vblua.document.layers()            -> [{id, name, kind, visible}]
vblua.document.get_layer(name)     -> layer row | nil (exact name)
vblua.document.create_layer({name, type?}) -> id (type default "image";
  idempotent by name: same kind returns existing id, different kind errors)
-- open/save/close/create: NOT scriptable (picker only, Phase 12+)
```

## `vblua.graph` — needs `document.write` for mutations

Target: active NodeEditor layer, else first. Kinds: `Blur LinearBlur
Brightness ColorChanger Zoom Equalizer Speed Value ExprX ExprXy ExprXyz
Frame Time Visualizer ChromaKey ApplyMask BackgroundBlur RegionsFromManual
PrivacyBlur DetectFace TrackMotion UnionImage2/3/5/7 GeoSize GeoPlacement
GeoRotate GeoTrapezoid GeoMirror GeoAdd VideoPlayer SepticPlayer
MouseEncoder OutputObject` (+ case/separator-insensitive aliases).
Denied (need Asset API): image/video/audio/fromapp/septic/mouse/param*.

```lua
id = vblua.graph.create(kind[, {layer, x, y, name}]) -> id
vblua.graph.nodes([layer]) -> [{id,name,kind,layer,x,y}]
vblua.graph.layers()       -> [{id,name,nodes,links}]
vblua.graph.get(id)        -> {id,name,kind,layer,links,params} | nil
vblua.graph.find(sub)      -> [ids] (name substring)
vblua.graph.rename(id, name) / remove(id) -> true
vblua.graph.duplicate(id[, offset=24]) -> new id (source pos + offset; links are NOT copied)
TimeOffset / FreezeFrame kinds (wire-driven like Speed; Reverse for rewind)
Transform (position/scale/rotation wires), Crop (rect/size wires, normalized),
FlipHorizontal / FlipVertical (mirror bits), TimeRemap (curve param, empty = identity)
vblua.graph.connect requires both nodes on the target layer (wires are intra-graph)
vblua.graph.ports(id) -> [{id, name, type, dir}] (never guess port ids)
vblua.graph.connect(from, out, to, in) -> true (ports/dirs/types/cycle now)
vblua.graph.disconnect(to, in) -> true
vblua.graph.set_param(id, key, number|string|curve) / get_param(id, key)
vblua.graph.params(id)     -> {key = type}
```
Params: Value.value, Expr*.expr (≤512), MouseEncoder.time_threshold/gain,
Visualizer.gain (clamped), TimeRemap.curve `{{t0,s0},{t1,s1},...}` (2..512 pts,
sorted/clamped/deduped on write; linear interp, ends hold). Effect amounts
arrive via wires, not fields.

## `vblua.timeline` — read-only playback state (no capability needed)

```lua
vblua.timeline.time()   -> seconds (frame / fps)
vblua.timeline.frame()  -> integer playhead frame (canonical)
vblua.timeline.fps()    -> project fps
vblua.timeline.range(s, e, "seconds"|"frames") -> true (declares this run's
  domain; validated; recorded for gating + panel; start<=end, >= 0)
Registry scripts (View > Timeline Scripts) evaluate on frame changes:
static runs once, temporal runs in-range, no undo entries, fresh runtime
per frame (deterministic). Panel shows ranges + ACTIVE @ playhead.
```

## `vblua.batch` — needs `document.write`

```lua
vblua.batch.grid({x,y}, cols 1..128, dx, dy, count 0..4096) -> [{x,y}]
vblua.batch.circle({x,y}, r, count[, phase]) -> [{x,y}]
vblua.batch.create_nodes(specs[, {layer,x,y}]) -> [ids]  (≤512/call)
vblua.batch.keyframes(id, track, {{frame,value[,interp]}...})  (≤2048/call)
```
Spec: `{kind, x?, y?, name?, params?}`. Canvas ids are NOT animation targets.

## `vblua.node` (templates) — needs `document.write`

```lua
vblua.node.define(name, {nodes={{kind,x?,y?,name?,params?}...},
                         links={{from,from_port,to,to_port}...}})  -- 1-based slots
vblua.node.spawn(name[, {layer,x,y}]) -> [ids]
vblua.node.templates() -> [names]; vblua.node.drop(name)
```
Validated at define (kinds/params/ports/cycles). 64 templates × 32 nodes.

## `vblua.animation` — needs `document.write`

Tracks: `pos_x pos_y rotation opacity color_r/g/b/a stroke_width
stroke_r/g/b/a geom_N param:*`. Interp: `linear` (default), `bezier`
(`smooth` aliases bezier; `step/cubic/custom` rejected — no host modes).

```lua
vblua.animation.set_keyframe(id, track, frame 0..1e6, value[, interp])
vblua.animation.remove_keyframe(id, track, frame) -> existed?
vblua.animation.keyframes(id, track) -> [{frame, value, interp}]
vblua.animation.sample(id, track, frame) -> number | nil (host-identical linear+bezier)
vblua.animation.tracks() / max_frame()  (span at fps=60)
```
Targets: canvas-node or layer ids. Stack spans not scriptable.

## `vblua.editor` — needs `document.write` (+ project entry for node ops)

```lua
vblua.editor.transaction(fn)  -- all-or-nothing; nested rejected
vblua.editor.nodes()          -> [{id,name,layer,kind,x,y,rotation,opacity}]
vblua.editor.get(id)          -> row | nil
vblua.editor.layer_nodes(layer) -> rows (layer id or name)
vblua.editor.rename_node(id, name) / duplicate_node(id)->id / delete_node(id)
vblua.editor.move_node(id, x, y)  -- absolute canvas px
vblua.editor.set_opacity(id, o)   -- clamped 0..1
```

## `vblua.kinematics` — pure (angles absolute radians, canvas 2D)

```lua
chain = vblua.kinematics.chain({l...})  -- 1..16 positive lengths
chain.solve({x,y}) -> [angles]; chain.fk(angles) -> [{x,y}] (base is [1])
vblua.kinematics.fk(lengths, angles[, base]) / ik(lengths, target[, {base,initial,iters}])
vblua.kinematics.ik2(l1,l2,base,target) -> {a1,a2} (clamps unreachable)
vblua.kinematics.damp(c,t,lambda,dt) / spring(p,v,t,k,c,dt)->{pos,vel}
vblua.kinematics.look_at(from,to)->angle / orbit(center,r,angle)->{x,y}
```

## `vblua.shader` — needs `document.write`

```lua
vblua.shader.validate(src) -> true (static host validator)
vblua.shader.create([layer[, name]]) -> id (DISABLED template)
vblua.shader.set_source(id, src)  -- validated now; ≤64 KiB
vblua.shader.set_uniforms(id, {...})  -- ≤64 finite floats
vblua.shader.set_enabled(id, bool) / rename / remove(id)
vblua.shader.passes([layer]) -> [{id,name,enabled,uniforms,source_len,has_error}]
```
GPU compile stays host-side (failures → pass `compile_error`, shown in UI).

## `vblua.video` — needs `document.write` (existing media only)

```lua
vblua.video.clips([layer]) / get(id)  -- {id,name,kind,start,length,offset,row,muted,locked,duration}; never paths
vblua.video.move(id, start) / trim(id, {offset,length}) / set_row(id, 0..64)
vblua.video.rename / remove(id)
vblua.video.split(id, at_sec) -> new id (strictly inside span)
vblua.video.set_muted(id, bool) / set_locked(id, bool) / duplicate(id)
vblua.video.ripple_delete(id) (remove + close the gap, same row)
vblua.video.markers() -> [{id, name, time}] (sorted)
vblua.video.add_marker(name, time) -> id / rename_marker / move_marker / remove_marker(id)
-- add_clip(path): denied (Asset API + picker). No speed/reverse (no host fields).
```

## `vblua.assets` — read-only

```lua
vblua.assets.list([kind]) -> [{ref, kind, name, width?, height?, bytes?, duration?}]
vblua.assets.info(ref) / kinds()
-- import(): denied (platform picker only). Handles: node:<uuid>, clip:<uuid>.
```

## `vblua.file` — needs `filesystem.read`

```lua
vblua.file.status() -> {picker = bool}
vblua.file.pick({filters={"png","jpg"}, title?}) -> image node id
```
Bytes hook→runtime (decode, 8 MiB/4096px caps, png/jpeg/bmp) → active (else
first) image layer. Mobile unwired → clean error.

## `vblua.ui` — needs `ui`

```lua
vblua.ui.panel(id, title) / drop_panel(id) / clear(id) / panels()
vblua.ui.text(panel, content ≤1024)
vblua.ui.button(panel, label, onclick) -> widget id
vblua.ui.slider(panel, label, min<max[, initial, onchange])
vblua.ui.checkbox(panel, label[, value, onchange])
```
Host renders in a floating window (32 panels × 128 widgets). Callbacks run
post-frame; errors become console lines.

## `vblua.image` / `vblua.kernel` — GPU runtime milestone 1 (no capability needed)

```lua
vblua.image.backend()  -- "gpu" | "cpu"
vblua.image.solid(w 1..4096, h[, {r,g,b,a}]) / from_asset(ref) / drop(id)
vblua.image.size(id) / luminance(id)->id / average_brightness(id)->0..1
vblua.image.average_color(id) / histogram(id)[256] / sample(id,u,v)
vblua.image.region_average(id,x,y,w,h) / grayscale / brightness(id,v) / contrast(id,v)
vblua.kernel.create({name, language="wgsl", source, parameters?}) -> id
vblua.kernel.info(id) / drop(id)
vblua.image.apply(frame, kernel[, {param = v}]) -> handle (GPU; cached pipeline)
```

## `vblua.debug` — tracing debugger (Phase 26, no capability needed)

```lua
vblua.debug.trace(fn)              -- run fn with line tracing (errors propagate)
vblua.debug.trace_events()         -- [{chunk, line}...] from the last trace
vblua.debug.breakpoint(chunk, line) / clear_breakpoints()
```

## `vblua.events` — none (subscribe) / callbacks need their own targets

`on(event, fn)` / `off(event)` / `list()` — events: `after_run`,
`node-created`, `node-deleted`, `addon-enabled`, `error`. One level only.

## `vblua.addons` — none (enable grants manifest permissions explicitly)

```lua
vblua.addons.dir() / refresh() / list() / info(id)
vblua.addons.enable(id) / disable(id) / reload(id) / uninstall(id)
```
Manifest: `{id, name, version, vblua="1"|{min,max}, permissions?, deps?}`.
