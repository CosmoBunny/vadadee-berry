-- kinematics.lua — FK/IK + motion helpers (Phase 8). Pure math, no document needed.
-- Angles are absolute canvas rotations (radians). Poses are returned —
-- write them with vblua.animation.set_keyframe (one undo entry).
local arm = vblua.kinematics.chain({ 100, 80, 60 })
local angles = arm.solve({ x = 150, y = 40 })
local joints = arm.fk(angles)
local tip = joints[#joints]
vblua.log(string.format("tip: %.1f, %.1f", tip.x, tip.y))

-- Analytic two-bone (clamps unreachable targets to full reach):
local s = vblua.kinematics.ik2(100, 80, { x = 0, y = 0 }, { x = 150, y = 40 })
vblua.log(string.format("shoulder=%.2f elbow=%.2f", s.a1, s.a2))

-- Follow/orbit/look-at helpers for per-frame Lua loops:
local eased = vblua.kinematics.damp(0, 10, 5, 1 / 60)
local p = vblua.kinematics.orbit({ x = 0, y = 0 }, 50, math.pi / 4)
return tostring(vblua.kinematics.look_at({ x = 0, y = 0 }, p) > 0)
