-- graph.lua — node graph API (Phase 5). Needs a NodeEditor layer + document.write.
-- Port ids are real: Blur has in(amount)/out; Value/Frame/Time have out.
local a = vblua.graph.create("Blur", { x = 10, y = 20 })
local b = vblua.graph.create("Blur", { x = 200, y = 20 })
vblua.graph.rename(a, "First")
vblua.graph.connect(a, "out", b, "in")
vblua.log("linked " .. a .. " -> " .. b)
-- vblua.graph.disconnect(b, "in")
-- local copy = vblua.graph.duplicate(a)
-- vblua.graph.remove(copy)
return vblua.graph.get(a).name
