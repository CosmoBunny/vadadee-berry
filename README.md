# Vadadee Berry

A creative tool built in Rust with egui and wgpu. It combines a vector graphics
editor, a video/audio timeline, a shader compositor, image editing, flowchart
diagramming, and a music sequencer — all in a single native application with a
GPU-accelerated canvas.

---

## Features

### Vector graphics

- Shapes: rectangles, ellipses, circles, polygons, arcs (chord and pie fill)
- Bezier paths with a full node editor (anchor, handle, symmetry modes)
- Brush strokes with pressure simulation
- Text objects with custom fonts
- Groups and layers with visibility, lock, z-order, and opacity
- Fill and stroke inspector: solid color, gradients, pattern
- Boolean path operations, corner fillets
- SVG import and export via usvg / resvg

### Image editing

- PNG, JPEG, BMP image objects embedded in the document
- Per-object opacity and blend modes
- Raster layer cache for composited output

### Video and timeline

- Video clip import via dynamically loaded FFmpeg (libavformat, libavcodec)
  — no FFmpeg subprocess, loaded at runtime with dlopen
- Supports mp4, mkv, webm and other formats FFmpeg can decode
- Timeline editor with multi-track AV clips, trim, offset, and sub-track rows
- Frame scrubbing and playback
- Export to MP4 (H.264/AAC) using libav directly — no subprocess

### Audio

- Audio clip support on the timeline (mp3, wav, aac, m4a, flac, ogg, opus, wma)
- Multi-track stereo mix with per-clip volume and timeline offset
- Mux mixed audio into the exported video
- Playback via rodio (WASAPI on Windows, CoreAudio on macOS, ALSA/PulseAudio on Linux)

### Shaders and GPU effects

- WGSL shader passes composited over the canvas via wgpu
- Built-in procedural shaders: blackhole (GPU and CPU), CRT, vignette
- Custom shading layers add post-process effects on top of vector content
- 4x MSAA rendering throughout

### Flowchart diagramming

- Flowchart node layer with rounded-rect nodes and labeled edges
- Orthogonal auto-routing for connector paths
- Obstacle-aware path layout with configurable margins and stubs

### Music sequencer

- Music clip layer with pitch/tick-based note data (piano roll style)
- Velocity, duration, and start position per note

### Animation

- Keyframe-based animation system via kramaframe
- Per-property keyframe interpolation
- Timeline scrubbing and frame-stepping
- Animated export to MP4

### Collaboration

- Real-time collaboration over TCP (LAN or relay)
- JSON-RPC MCP server built in: external AI clients can query and control
  the editor over TCP (port 17345 by default) or stdin/stdout
  (`vadadee-mcp-stdio` binary)

### Platform

| Platform | Renderer       | Window backend            |
|----------|----------------|---------------------------|
| Linux    | wgpu / Vulkan  | Wayland + X11 (eframe)    |
| macOS    | wgpu / Metal   | AppKit (eframe)           |
| Windows  | wgpu / DX12    | Win32 (eframe)            |
| Android  | wgpu / Vulkan  | GameActivity (eframe)     |

---

## Building

### Requirements

- Rust 1.92 or newer: https://rustup.rs

---

### Linux

System dependencies (Ubuntu / Debian):

```bash
sudo apt install \
  libgtk-3-dev \
  libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
  libxkbcommon-dev \
  libwayland-dev \
  libasound2-dev \
  pkg-config
```

Build and run:

```bash
cargo build --release --bin vadadee-berry
./target/release/vadadee-berry
```

---

### macOS

Native build on a Mac (requires Xcode Command Line Tools):

```bash
xcode-select --install
cargo build --release --bin vadadee-berry --bin vadadee-mcp-stdio
```

Binaries are written to `target/release/`.

Cross-compile from Linux (requires a macOS SDK — see https://github.com/tpoechtrager/osxcross):

```bash
cargo install cargo-zigbuild
rustup target add aarch64-apple-darwin

export SDKROOT=/path/to/MacOSX11.3.sdk
export MACOSX_DEPLOYMENT_TARGET=11.3
export RUSTFLAGS='-C link-arg=-undefined -C link-arg=dynamic_lookup'

cargo zigbuild --target aarch64-apple-darwin --release \
  --bin vadadee-berry --bin vadadee-mcp-stdio
```

The `-undefined dynamic_lookup` flag is required because the cpal audio library
references CoreAudio loopback symbols that are only present on macOS 14.2+.
On older systems the linker defers resolution and the loopback code path remains
unreachable.

---

### Windows

Native build on Windows (requires MSVC build tools):

```bash
cargo build --release --bin vadadee-berry
```

Cross-compile from Linux:

```bash
cargo install cross
cross build --target x86_64-pc-windows-gnu --release --bin vadadee-berry
```

The `Cross.toml` in the repository root points to the public
`ghcr.io/cross-rs/x86_64-pc-windows-gnu:edge` Docker image automatically.

If you see "permission denied" connecting to the Docker socket:

```bash
sg docker -c "cross build --target x86_64-pc-windows-gnu --release --bin vadadee-berry"
# or activate the docker group in the current shell:
newgrp docker
```

Output: `target/x86_64-pc-windows-gnu/release/vadadee-berry.exe`

---

### Android

Requirements:

- Android SDK and NDK r25c or newer (side-by-side NDK)
- JDK 17 or newer
- `cargo-ndk`: `cargo install cargo-ndk`
- `ANDROID_HOME` and `ANDROID_NDK_HOME` set

The Gradle dependency `androidx.games:games-activity` must be 4.4.0.
Older 2.x versions crash at startup with `NoSuchMethodError: onTouchEventNative`.

Build debug APK:

```bash
./gradlew assembleDebug
```

Install on a connected device:

```bash
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

Release APK (requires a signing keystore configured in `local.properties`):

```
RELEASE_STORE_FILE=/path/to/keystore.jks
RELEASE_STORE_PASSWORD=yourpassword
RELEASE_KEY_ALIAS=youralias
RELEASE_KEY_PASSWORD=yourkeypassword
```

```bash
./gradlew assembleRelease
```

---

## Video export dependency

Video import and export require FFmpeg shared libraries at runtime.
The application loads them dynamically (dlopen) — no static linking, no subprocess.

Supported library versions: libavformat 60-61, libavcodec 60-61, libavutil 58-59.

On Linux: `sudo apt install ffmpeg` (or equivalent).
On macOS: `brew install ffmpeg`.
On Windows: place the DLLs (`avformat-61.dll`, `avcodec-61.dll`, `avutil-59.dll`) next to the executable.

If FFmpeg is not found the editor still runs; video import/export is silently disabled.

---

## MCP server

The `vadadee-mcp-stdio` binary exposes the editor over the Model Context Protocol
on stdin/stdout, usable by AI tools and IDE extensions. The built-in TCP MCP
server runs on port 17345 (override with `VADADEE_MCP_PORT`).

---

## Project file format

Projects are saved as `.vadadee-berry.json`. SVG can be imported and exported
via File > Import SVG / Export SVG.

---

## License

MIT