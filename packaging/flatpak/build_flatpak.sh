#!/usr/bin/env bash
# =============================================================================
# build_flatpak.sh — stage files and build the Flatpak bundle.
#
# Staging (single source of truth reused from Linux packaging):
#   dist/flatpak/stage/
#   ├── bin/vadadee-berry                     (release binary)
#   ├── com.cosmobunny.VadadeeBerry.desktop   (copied from packaging/linux/)
#   ├── com.cosmobunny.VadadeeBerry.metainfo.xml (this dir)
#   └── icons/hicolor/<size>/apps/
#         com.cosmobunny.VadadeeBerry.png     (copied from packaging/linux/)
#
# Then: flatpak-builder (repo) + flatpak build-bundle (.flatpak).
# Needs: flatpak, flatpak-builder, the freedesktop 24.08 runtime/SDK
# (network on first use). Fails early with clear messages otherwise.
#
# Usage:
#   ./packaging/flatpak/build_flatpak.sh [--bin path/to/vadadee-berry]
#   ./packaging/flatpak/build_flatpak.sh --stage-only [--bin ...]
#     assemble + validate dist/flatpak/stage without invoking flatpak-builder
#     (works offline; used for CI debugging and layout checks).
# Output:
#   release/VadadeeBerry-linux-<arch>.flatpak (+ .sha256)
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FLATPAK_DIR="$SCRIPT_DIR"
LINUX_DIR="$(cd "$SCRIPT_DIR/../linux" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$ROOT"

APP_ID="com.cosmobunny.VadadeeBerry"
BIN=""
STAGE_ONLY=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --bin) BIN="$2"; shift 2 ;;
    --stage-only) STAGE_ONLY=1; shift ;;
    -h|--help) sed -n '2,22p' "$0"; exit 0 ;;
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
  x86_64|amd64) ARCH_TAG="x86_64" ;;
  aarch64|arm64) ARCH_TAG="aarch64" ;;
  *) echo "ERROR: unsupported arch for Flatpak bundle: $ARCH" >&2; exit 1 ;;
esac

for tool in flatpak flatpak-builder; do
  if [[ "$STAGE_ONLY" -eq 1 ]]; then
    break
  fi
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "ERROR: missing required tool: $tool" >&2
    echo "Install Flatpak tooling first (e.g. apt install flatpak flatpak-builder)." >&2
    exit 1
  fi
done

STAGE="$ROOT/dist/flatpak/stage"
RELEASE_DIR="$ROOT/release"
OUT="$RELEASE_DIR/VadadeeBerry-linux-${ARCH_TAG}.flatpak"

echo "▶ Staging Flatpak inputs"
rm -rf "$STAGE"
mkdir -p "$STAGE/bin"
cp "$BIN" "$STAGE/bin/vadadee-berry"
chmod +x "$STAGE/bin/vadadee-berry"
cp "$LINUX_DIR/$APP_ID.desktop" "$STAGE/$APP_ID.desktop"
cp "$FLATPAK_DIR/$APP_ID.metainfo.xml" "$STAGE/$APP_ID.metainfo.xml"
mkdir -p "$STAGE/icons"
cp -r "$LINUX_DIR/icons/hicolor" "$STAGE/icons/hicolor"

echo "▶ Validating desktop entry, metainfo and icons"
if command -v desktop-file-validate >/dev/null 2>&1; then
  desktop-file-validate "$STAGE/$APP_ID.desktop"
else
  echo "(desktop-file-validate not installed — skipping)"
fi
if command -v appstreamcli >/dev/null 2>&1; then
  # Informational only: developer-id/releases pedantic notes are expected
  # pre-Flathub-submission and must not fail the build.
  appstreamcli validate --no-net "$STAGE/$APP_ID.metainfo.xml" || true
else
  echo "(appstreamcli not installed — skipping)"
fi
for s in 16 32 48 64 128 256 512; do
  f="$STAGE/icons/hicolor/${s}x${s}/apps/$APP_ID.png"
  test -f "$f" || { echo "ERROR: missing icon $f" >&2; exit 1; }
done

if [[ "$STAGE_ONLY" -eq 1 ]]; then
  echo "Stage-only mode: validated stage at $STAGE (no flatpak-builder run)."
  exit 0
fi

echo "▶ Building Flatpak repo (needs runtime/SDK, network on first use)"
BUILD_DIR="$ROOT/dist/flatpak/build"
REPO_DIR="$ROOT/dist/flatpak/repo"
flatpak-builder --force-clean --repo="$REPO_DIR" "$BUILD_DIR" \
  "$FLATPAK_DIR/$APP_ID.yml"

echo "▶ Bundling $OUT"
mkdir -p "$RELEASE_DIR"
rm -f "$OUT"
flatpak build-bundle "$REPO_DIR" "$OUT" "$APP_ID"
chmod +x "$OUT" 2>/dev/null || true

if command -v sha256sum >/dev/null 2>&1; then
  (cd "$RELEASE_DIR" && sha256sum "$(basename "$OUT")" > "$(basename "$OUT").sha256")
fi

echo " Done: $OUT"
