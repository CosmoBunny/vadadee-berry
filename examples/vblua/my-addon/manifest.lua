-- my-addon/manifest.lua — addon manifest (Phase 18).
-- Place the folder in <data-dir>/addons/, then in Lua:
--   vblua.addons.refresh()
--   vblua.addons.enable("example.myaddon")
return {
  id = "example.myaddon",
  name = "My Addon",
  version = "1.0.0",
  vblua = "1", -- or { min = "1", max = "1" }
  permissions = { "document.write", "ui" },
  deps = {}, -- exact addon ids, enabled first
  description = "Example addon: one panel, one template.",
}
