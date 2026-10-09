-- procedural.lua — batch generation (Phase 15). Needs document.write.
-- One Lua call stages a whole batch: 100 nodes + full keyframe tracks.
local pts = vblua.batch.grid({ x = 0, y = 0 }, 10, 60, 60, 100)
local specs = {}
for i, p in ipairs(pts) do
  specs[i] = { kind = "Value", x = p.x, y = p.y, params = { value = i } }
end
local ids = vblua.batch.create_nodes(specs)
vblua.log("created " .. #ids)
-- local ring = vblua.batch.circle({ x = 400, y = 300 }, 150, 12)
-- vblua.batch.keyframes(NODE_ID, "rotation", { { 0, 0 }, { 60, 360 } })
return #ids
