# Session handoff (2026-08-02)

## Do next session
1. **Smoke-test** Paint tab + float + brush engine in the app (user was blocked on unfinished UI).
2. If float undo is double-stacking (hole cut + apply both in history), collapse to one undo step.
3. Optional: richer float (live handles on canvas), not only Paint-tab buttons.

## Shipped / in working tree (commit if not yet)
### A — git hygiene / Grok uploads
- `~/.grok/config.toml`: `[tools] respect_gitignore = true` (stops multi‑GB `target/` from being scanned/uploaded).
- Project: `.cursorignore`, extra `target/` in `.gitignore`.

### B — Paint layer workflow
- Public APIs: `raster_new_paint_layer`, `raster_new_paint_layer_from_selection`, `raster_paint_target_info`.
- Auto-create still via `ensure_raster_paint_target` on first stroke.
- **Dedicated action tab `ActionTab::Paint`** (not crammed into Geometry / tool strip).
- Raster tools (K/X/F/U/W) promote **Paint** tab.
- Tool strip is slim: size/hardness/opacity/spacing + “Open Paint tab…”.
- Paint tab holds: new layers, clear, mask ops, float transform, brush engine, symmetry.

### C — Brush engine
- Session fields: `flow`, `scatter`, `size_jitter`, `angle_deg`, `angle_jitter`, `aspect`.
- Presets updated (incl. Spray); apply sets engine fields.
- `stamp_tip_masked` (ellipse aspect + rotation); jitter/scatter in `raster_stamp_at`.
- UI: Paint tab → Brush engine section.

### D — Float selection transform
- `FloatingPixels` on `RasterSession`.
- `raster_float_selection` / `apply` / `cancel` / `nudge` / `scale` / `rotate` / `float_pointer`.
- Canvas drag moves float when active (paint + raster select tools).
- UI: Paint tab → Float selection.

## Key files
- `src/ui.rs` — `ActionTab::Paint`, `paint_section`, slim raster tool strip
- `src/app.rs` — paint layer + float APIs, stamp engine wiring
- `src/tools/mod.rs` — `RasterSession` / presets / `FloatingPixels`
- `src/raster/mod.rs` — `stamp_tip_masked`, `brush_unit_noise`

## Known pitfalls
- MCP `vadadee-berry` may fail to connect if app not running (port 17345).
- Do **not** upload `target/` (respect_gitignore must stay true).
- Untracked junk: `_snap_hex.png`, `_snap_hex2.png` — do not commit.
- Branch was `main`; prefer commit+push only when user asks (they often do).

## Earlier fixed (already on origin earlier)
- Pixel brush SE hold drift (`pixel_last_doc`)
- Font double-click panic (unbound FontFamily::Name)
- Thin shape min size + export DPI (compact DPI UI, no preset chips)
- Paste/duplicate zoom-aware nudge
