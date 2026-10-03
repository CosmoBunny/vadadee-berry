-- document.lua — read the current document, stage batched writes (Phase 4).
-- Reads come from a snapshot; writes queue as ONE batch applied by Rust
-- after the script returns (one undo transaction).
local d = vblua.document.current()
if d == nil then
  vblua.log("no document open")
  return nil
end
vblua.log("doc: " .. d.name .. " " .. d.width .. "x" .. d.height)
vblua.log("layers: " .. d.layer_count)
-- Staged (needs document.write capability); applied after return:
-- vblua.document.rename("My Title")
-- vblua.document.resize(1920, 1080)
return d.name
