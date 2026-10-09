-- video.lua — AV clip orchestration (Phase 10). Needs document.write.
-- Only EXISTING clips are touched: no paths in, no paths out.
-- add_clip(path) is denied (Asset API, Phase 11); speed/reverse/transitions
-- don't exist on the host clip model yet.
for _, c in ipairs(vblua.video.clips()) do
  vblua.log(string.format("%s [%s] start=%.1f len=%.1f row=%d", c.name, c.kind, c.start, c.length, c.row))
end
-- local id = vblua.video.clips()[1].id
-- vblua.video.move(id, 12.5)
-- vblua.video.trim(id, { offset = 2.0, length = 8.0 })
-- local right = vblua.video.split(id, 14.0)
-- vblua.video.set_row(id, 1)
-- vblua.video.rename(id, "Intro")
-- vblua.video.remove(id)
return #vblua.video.clips()
