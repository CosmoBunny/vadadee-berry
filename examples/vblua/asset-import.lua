-- assets.lua — project asset inventory (Phase 11). Read-only, no capability needed.
-- Handles are opaque (node:<uuid> / clip:<uuid>). Paths and bytes never cross.
-- import() is denied: new bytes enter via the platform picker (Phase 12).
for _, a in ipairs(vblua.assets.list()) do
  vblua.log(string.format("%s [%s] %s", a.ref, a.kind, a.name))
end
vblua.log("kinds: " .. table.concat(vblua.assets.kinds(), ", "))
-- local info = vblua.assets.info(vblua.assets.list("image")[1].ref)
return #vblua.assets.list()
