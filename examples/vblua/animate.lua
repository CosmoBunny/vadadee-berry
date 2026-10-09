-- animate.lua — keyframe animation (Phase 7). Needs document.write.
-- Tracks: pos_x, pos_y, rotation, opacity, color_r/g/b/a, stroke_*,
-- geom_N, param:*. Interpolation: linear (default) or bezier.
-- Uses the first canvas node (pass NODE_ID to target a specific one).
local id = NODE_ID or ((vblua.editor.nodes()[1] or {}).id)
assert(id, "no canvas nodes — draw something first")
vblua.animation.set_keyframe(id, "rotation", 0, 0)
vblua.animation.set_keyframe(id, "rotation", 30, 360, "bezier")
vblua.animation.set_keyframe(id, "pos_x", 0, 0)
vblua.animation.set_keyframe(id, "pos_x", 60, 500)
vblua.log("mid-spin: " .. tostring(vblua.animation.sample(id, "rotation", 15)))
vblua.log("span frames: " .. vblua.animation.max_frame())
return #vblua.animation.keyframes(id, "rotation")
