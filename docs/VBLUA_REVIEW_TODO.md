# VBLua Review TODO (reviewer: layers/objects/timeline direction)

Standing constraints this review interacts with (never lifted):
- Same Lua on all platforms; Lua never touches Rust internals, GPU objects, paths.
- Staged command batches; one script run = one undo entry per store.
- Nothing committed without explicit order.

`vb` already exists as an alias for `vblua` (`api.rs:347`) — no rename needed.

## Verdicts per review section

### Already satisfied — no work
- §5 (selection must not be primary access): the scripting API has NO
  selection path at all. Document/graph/video/animation all traverse
  layers/nodes/clips directly. Nothing to remove.
- §13 second half (drive Speed/ZoomVideo from Lua): covered — `graph.create`
  + `connect` + `set_param` + animation keyframes already script node graphs.

### Incremental additions — no conflicts (propose as R1)
- §1/§2 (`create_layer` idempotent by name, `get_layer`): does not exist.
  Build id-based (returns id, staged command): create matches by name —
  compatible type returns existing id, incompatible type errors.
  Needs: `DocumentCommand::{CreateLayer, ...}` + apply + snapshot + tests.
- §3/§4/§6 (layer traversal + object iteration/mutation): no layer-object
  API exists today (document API is title/resize/visibility only).
  Build WITHOUT live userdata (see conflict R0): `document.layers()`,
  `layer.objects(id)` snapshot rows, `object.get/set_*` via staged commands,
  plus a Lua-side `iter()` helper style over snapshot rows. Same ergonomics,
  sandbox intact.
- §8/`timeline.time/frame/fps` reads: new read-only API over host
  playback state (no declared-range semantics until R3).

### Conflicts — need explicit user decision (R0)
- §3/§4 "live userdata, mutate the actual editor object": violates the
  sandbox + staged-batch + one-undo-per-run model. Proposal: keep the
  command-queue model (ids + optimistic snapshots, as graph/video already
  do) and expose `layer:iter()` ergonomics over snapshots. If the user
  insists on live handles, that is a full architecture change (new
  mutation path, per-call undo coalescing, mobile parity re-proof).
- §7–§11 (timeline evaluation: scripts re-run per frame over declared
  ranges + Lua panel): genuinely new subsystem. Needs: script registry
  (persisted where?), playback-loop evaluation hooks, per-frame budgets,
  undo story (evaluations MUST NOT flood history — evaluate without
  pushing undo, commit only on explicit save/apply?), determinism rules,
  and the panel UI (§11 depends on all of it). Largest item here.

## Proposed build order (pending user sign-off on R0)
- [x] R0 — decision: snapshot+commands ergonomics vs live userdata (chose snapshot+commands)
- [x] R1a — `document.layers()` + `create_layer` (idempotent by name) + `get_layer` (implemented, uncommitted)
- [x] R1b — layer object rows + object get/set via staged commands (implemented, uncommitted)
- [x] R2 — `timeline.time/frame/fps` reads (playback state, no eval yet) (implemented, uncommitted)
- [x] R3a — `timeline.range` declaration + per-frame evaluation runtime (implemented, uncommitted)
- [x] R3b — Lua timeline panel (ranges, active @ playhead) (implemented, uncommitted)
- [ ] R4 — tests per review list (layers, dup prevention, iter, mutation,
        empty/missing layers, frames/seconds, boundaries, repeated eval)

## R3 design (locked before build)
- Registry: `Document.timeline_scripts: Vec<TimelineScript{id,name,source,enabled}>`
  (persisted, undoable CRUD; source cap 64 KiB, max 32 scripts).
- `timeline.range(s,e,unit)` declares this run's domain (validated, recorded
  runtime-side, NOT a document write). `time()/frame()/fps()` reads mark the
  script temporal (host `timeline_touched` flag).
- Gating per frame: static (never touched time) runs once; temporal runs when
  the playhead is inside its last declared range (seconds converted at current
  fps; frames direct); undeclared temporal runs once to learn.
- Eval: fresh permissive runtime per frame (no cross-talk), all active scripts
  executed, one `apply_drained` with NO history push (playback effects, like
  animation sampling — scripts must be functions of the playhead).
- Hook: same frame-change site as animation apply (skips live drags).
- Trust: auto-runs on playback while scripts exist (experimental channel);
  per-script enable toggle is the control. Only open trusted projects.
- Panel: list + recorded range + ACTIVE badge + enable/delete + add-current.
- Console Run declaring `timeline.range()` auto-registers the script text as
  the "Console" registry entry (upsert, undo-coalesced) — no panel trip.
