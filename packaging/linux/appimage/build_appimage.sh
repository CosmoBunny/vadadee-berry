#!/usr/bin/env bash
# =============================================================================
# build_appimage.sh — assemble VadadeeBerry.AppDir and build the AppImage.
#
# Layout (per Linux packaging design):
#   VadadeeBerry.AppDir/
#   ├── AppRun                                        (packaging/linux/AppRun)
#   ├── vadadee-berry                                (release binary)
#   ├── com.cosmobunny.VadadeeBerry.desktop          (shared desktop entry)
#   └── usr/share/icons/hicolor/<size>/apps/
#         com.cosmobunny.VadadeeBerry.png            (shared icon tree)
#
# Then packs with linuxdeploy (downloaded if missing — needs network).
# No manual AppImage hacking: linuxdeploy consumes the .desktop + icon tree.
#
# Usage:
#   ./packaging/linux/appimage/build_appimage.sh [--bin path/to/vadadee-berry]
# Output:
#   release/VadadeeBerry-linux-<arch>.AppImage (+ .sha256)
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# SCRIPT_DIR = packaging/linux/appimage → repo root is three levels up.
LINUX_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
cd "$ROOT"

APP_ID="com.cosmobunny.VadadeeBerry"
BIN="${1:-}"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --bin) BIN="$2"; shift 2 ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "Unknown arg: $1" >&2; exit 1 ;;
  esac
done
if [[ -z "$BIN" ]]; then
  BIN="$ROOT/target/release/vadadee-berry"
fi
if [[ ! -x "$BIN" ]]; then
  echo "ERROR: studio binary not found/executable: $BIN" >&2
  echo "Build it first (cargo build --release --bin vadadee-berry)." >&2
  exit 1
fi

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64) LINUXDEPLOY_ARCH="x86_64"; ARCH_TAG="x86_64" ;;
  aarch64|arm64) LINUXDEPLOY_ARCH="aarch64"; ARCH_TAG="aarch64" ;;
  *) echo "ERROR: unsupported arch for AppImage: $ARCH" >&2; exit 1 ;;
esac

WORK="$ROOT/dist/appimage"
APPDIR="$WORK/VadadeeBerry.AppDir"
RELEASE_DIR="$ROOT/release"
OUT="$RELEASE_DIR/VadadeeBerry-linux-${ARCH_TAG}.AppImage"

echo "▶ Assembling $APPDIR"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/share/icons"
cp "$BIN" "$APPDIR/vadadee-berry"
chmod +x "$APPDIR/vadadee-berry"
cp "$LINUX_DIR/AppRun" "$APPDIR/AppRun"
chmod +x "$APPDIR/AppRun"
cp "$LINUX_DIR/$APP_ID.desktop" "$APPDIR/$APP_ID.desktop"
cp -r "$LINUX_DIR/icons/hicolor" "$APPDIR/usr/share/icons/hicolor"
# Top-level icon symlink is conventional for older tooling.
ln -sfn "usr/share/icons/hicolor/256x256/apps/$APP_ID.png" "$APPDIR/$APP_ID.png"
ln -sfn "usr/share/icons/hicolor/256x256/apps/$APP_ID.png" "$APPDIR/.DirIcon"

echo "▶ Validating desktop entry + icons"
if command -v desktop-file-validate >/dev/null 2>&1; then
  desktop-file-validate "$APPDIR/$APP_ID.desktop"
else
  echo "(desktop-file-validate not installed — skipping)"
fi
for s in 16 32 48 64 128 256 512; do
  f="$APPDIR/usr/share/icons/hicolor/${s}x${s}/apps/$APP_ID.png"
  test -f "$f" || { echo "ERROR: missing icon $f" >&2; exit 1; }
done

echo "▶ Packing AppImage (linuxdeploy)"
LINUXDEPLOY="$WORK/linuxdeploy-$LINUXDEPLOY_ARCH.AppImage"
if [[ ! -x "$LINUXDEPLOY" ]]; then
  echo "Downloading linuxdeploy (needs network)…"
  URL="https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-${LINUXDEPLOY_ARCH}.AppImage"
  if ! curl -fL -o "$LINUXDEPLOY" "$URL"; then
    echo "ERROR: could not download linuxdeploy from $URL" >&2
    echo "Provide it manually at $LINUXDEPLOY (executable)." >&2
    exit 1
  fi
  chmod +x "$LINUXDEPLOY"
fi
mkdir -p "$RELEASE_DIR"
rm -f "$OUT"
# linuxdeploy names the output itself (<Name>-<arch>.AppImage); normalize after.
# (Exclude the linuxdeploy binary itself from the glob.)
(cd "$WORK" && ARCH="$LINUXDEPLOY_ARCH" "$LINUXDEPLOY" --appdir "$APPDIR" --output appimage)
shopt -s nullglob
CANDIDATES=()
for f in "$WORK"/VadadeeBerry*.AppImage "$WORK"/*.AppImage; do
  case "$(basename "$f")" in
    linuxdeploy-*) continue ;;
    *) CANDIDATES+=("$f") ;;
  esac
done
shopt -u nullglob
if [[ ${#CANDIDATES[@]} -eq 0 ]]; then
  echo "ERROR: linuxdeploy produced no AppImage" >&2
  exit 1
fi
mv -f "${CANDIDATES[0]}" "$OUT"
chmod +x "$OUT"

if command -v sha256sum >/dev/null 2>&1; then
  (cd "$RELEASE_DIR" && sha256sum "$(basename "$OUT")" > "$(basename "$OUT").sha256")
fi

echo " Done: $OUT"
