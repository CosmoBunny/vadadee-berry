# Vadadee Berry

Vector graphics editor built with **egui** and GPU rendering via **wgpu** (through eframe).

| Concept | Implementation |
|---------|----------------|
| SVG document / object tree | `document` module: layers, groups, shapes, paths |
| Paths | `kurbo` beziers + `lyon` tessellation |
| Canvas + tools | `canvas` + `tools` (select, node, rect, ellipse, pen) |
| Desktop UI | egui panels: toolbox, layers, fill/stroke, status |
| File I/O | `.vadadee-berry.json` native project + SVG import/export (`usvg` / `resvg`) |
| Undo | `undo` crate command stack |

## Run

```bash
cargo run -p vadadee-berry
```

## UI

Dark, accent-blue chrome: top bar (file/edit/view), vertical tool rail, right **inspector** (document, layers, objects, appearance, geometry), status bar with cursor + zoom.

## Editing

- **Layers**: add, rename, visibility, lock, active layer; object list with z-order nudges
- **Inspector**: live fill/stroke/opacity, rect/ellipse numeric geometry, object name
- **Canvas**: 8-handle resize (single selection), move with undo on release, Shift+click multi-select
- **Node tool**: drag path points with undo

## Shortcuts

| Key | Action |
|-----|--------|
| V / N / R / E / P | Tools |
| Wheel | Zoom (Ctrl = fine) |
| Space / middle drag | Pan |
| Del | Delete |
| Ctrl+Z / Y | Undo / redo |
| Ctrl+D | Duplicate |
| Ctrl+C / V / X | Copy / paste / cut |
| Ctrl+O / S | Open SVG / save project |
| Enter | Finish pen path |

## License

MIT