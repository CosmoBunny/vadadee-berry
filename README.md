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

## Run (desktop)

```bash
cargo run -p vadadee-berry
```

## Android (native, no WebView)

The mobile build uses **eframe + wgpu** via **GameActivity** (Vulkan/GLES), not a WebView or WASM shell.

**Requirements:** Android SDK + NDK (side-by-side), `cargo-ndk`, JDK 17+.

The Gradle dependency `androidx.games:games-activity` must be **4.4.0** to match the
`android-activity` glue used by eframe/winit (older 2.x versions crash at startup with
`NoSuchMethodError: onTouchEventNative`).

```bash
./scripts/build-android.sh
```

Output: `app/build/outputs/apk/debug/app-debug.apk`

Install on a device:

```bash
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

File open/save dialogs are desktop-only for now; the editor UI and canvas run on device.

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