-- addon.lua — scripted node type / template recipe (Phase 14).
-- Lua describes; the engine validates at define and owns the recipe.
-- Spawned nodes are ordinary host nodes (one batch, one undo entry).
vblua.node.define("SoftGlow", {
  nodes = {
    { kind = "Blur", x = 0, y = 0 },
    { kind = "Brightness", x = 200, y = 0, name = "Lift" },
  },
  links = {
    { from = 1, from_port = "out", to = 2, to_port = "in" },
  },
})

local ids = vblua.node.spawn("SoftGlow", { x = 100, y = 100 })
vblua.log("spawned " .. #ids .. " nodes")
-- vblua.node.templates() / vblua.node.drop("SoftGlow")
return #ids
