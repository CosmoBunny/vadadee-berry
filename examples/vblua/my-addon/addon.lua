-- my-addon/addon.lua — runs on enable (Phase 18).
-- Declares (panels, templates); the engine owns everything after.
vblua.ui.panel("myaddon", "My Addon")

vblua.ui.button("myaddon", "Soft Glow", function()
  local ids = vblua.node.spawn("SoftGlow", { x = 60, y = 60 })
  vblua.log("spawned " .. #ids)
end)

vblua.node.define("SoftGlow", {
  nodes = {
    { kind = "Blur", x = 0, y = 0 },
    { kind = "Brightness", x = 200, y = 0 },
  },
  links = {
    { from = 1, from_port = "out", to = 2, to_port = "in" },
  },
})
