-- picker.lua — platform file picker abstraction (Phase 12). Needs document.write
-- + filesystem.read. Desktop opens rfd; mobile reports picker=false until
-- SAF / document picker bridges land. Same script everywhere.
local st = vblua.file.status()
vblua.log("picker wired: " .. tostring(st.picker))
if not st.picker then
  vblua.log("no platform picker — skipping")
  return nil
end
-- Returns the new image node id; bytes never touch Lua (hook -> runtime).
local id = vblua.file.pick({ filters = { "png", "jpg" } })
local info = vblua.assets.info("node:" .. id)
vblua.log(string.format("imported %s %.0fx%.0f", info.name, info.width, info.height))
return id
