-- custom-tool.lua — UI scripting (Phase 13). Needs document.write + ui.
-- Declares an extension panel (host renders it in "VBLua Panels").
-- The host stays in charge: panels are floating windows, never UI replacement.
vblua.ui.panel("tools", "My Tool")
vblua.ui.text("tools", "Generate helpers")

vblua.ui.button("tools", "Add Blur", function()
  local id = vblua.graph.create("Blur", { x = 40, y = 40 })
  vblua.log("created " .. id)
end)

vblua.ui.slider("tools", "Radius", 0, 100, 12, function(v)
  vblua.log("radius " .. v)
end)

vblua.ui.checkbox("tools", "Enabled", true, function(on)
  vblua.log("enabled: " .. tostring(on > 0))
end)

return #vblua.ui.panels()
