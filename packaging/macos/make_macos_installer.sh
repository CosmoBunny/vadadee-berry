#!/usr/bin/env bash
# =============================================================================
# make_macos_installer.sh
# Assembles "Vadadee Berry.app" and packages a distributable .dmg (or .zip).
#
# Usage:
#   ./packaging/macos/make_macos_installer.sh [--arch aarch64|x86_64] [--zip]
#       [--bin-dir path/to/release/bins]
#
# Binary sources (in order):
#   1. --bin-dir (CI macOS job: target/release from the desktop build step)
#   2. target/<arch>-apple-darwin/release (cross-compile via docker/zigbuild)
#
# Icon: packaging/macos/vadadee-berry.icns (committed, same master as every
# other format). Regenerated inline with Pillow only if the committed file is
# missing.
#
# DMG backends (in order): hdiutil (native macOS) → create-dmg →
# genisoimage/mkisofs → ZIP fallback. Bundle ID stays com.vadadee.berry
# (existing installed identity — do not rename silently).
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$ROOT"

ARCH="aarch64"
FORCE_ZIP=0
BIN_DIR=""
APP_NAME="Vadadee Berry"
BUNDLE_ID="com.vadadee.berry"
DIST_DIR="$ROOT/dist/macos"

VERSION="$(
  sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1
)"
VERSION="${VERSION:-0.0.2}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --arch) ARCH="$2"; shift 2 ;;
    --zip)  FORCE_ZIP=1; shift ;;
    --bin-dir) BIN_DIR="$2"; shift 2 ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "Unknown arg: $1"; exit 1 ;;
  esac
done

TARGET="${ARCH}-apple-darwin"
if [[ -z "$BIN_DIR" ]]; then
  BIN_DIR="$ROOT/target/$TARGET/release"
fi
APP_DIR="$DIST_DIR/${APP_NAME}.app"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo " Building: $APP_NAME v$VERSION"
echo " Arch    : $ARCH  ($TARGET)"
echo " Bins    : $BIN_DIR"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

for bin in vadadee-berry vadadee-mcp-stdio; do
  if [[ ! -f "$BIN_DIR/$bin" ]]; then
    echo "✗ Missing binary: $BIN_DIR/$bin" >&2
    echo "  Native macOS: cargo build --release, then rerun with --bin-dir target/release" >&2
    echo "  Cross Linux : sg docker -c \"docker run --rm -v \$PWD:/io -w /io \\" >&2
    echo "         -e SDKROOT=/opt/MacOSX11.3.sdk -e MACOSX_DEPLOYMENT_TARGET=11.3 \\" >&2
    echo "         -e RUSTFLAGS='-C link-arg=-undefined -C link-arg=dynamic_lookup' \\" >&2
    echo "         krama-mac-builder:latest \\" >&2
    echo "         cargo zigbuild --target $TARGET --release --bin vadadee-berry --bin vadadee-mcp-stdio\"" >&2
    exit 1
  fi
done

echo
echo "▶ Assembling .app bundle…"
rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources"

cp "$BIN_DIR/vadadee-berry"     "$APP_DIR/Contents/MacOS/vadadee-berry"
cp "$BIN_DIR/vadadee-mcp-stdio" "$APP_DIR/Contents/MacOS/vadadee-mcp-stdio"
chmod +x "$APP_DIR/Contents/MacOS/vadadee-berry"
chmod +x "$APP_DIR/Contents/MacOS/vadadee-mcp-stdio"

cp "$SCRIPT_DIR/Info.plist" "$APP_DIR/Contents/Info.plist"
echo -n "APPL????" > "$APP_DIR/Contents/PkgInfo"

# ── Icon: committed file first, Pillow fallback second ───────────────────────
ICNS_OUT="$APP_DIR/Contents/Resources/AppIcon.icns"
if [[ -f "$SCRIPT_DIR/vadadee-berry.icns" ]]; then
  echo "▶ Using committed icon: packaging/macos/vadadee-berry.icns"
  cp "$SCRIPT_DIR/vadadee-berry.icns" "$ICNS_OUT"
else
  ICON_SRC="$ROOT/assets/icon_studio_1024.png"
  [[ -f "$ICON_SRC" ]] || ICON_SRC="$ROOT/assets/vadadee_berry_icon.png"
  if [[ -f "$ICON_SRC" ]] && python3 -c "import PIL.Image" 2>/dev/null; then
    echo "▶ Generating AppIcon.icns from $ICON_SRC…"
    ICON_SRC="$ICON_SRC" ICNS_OUT="$ICNS_OUT" python3 - <<'PYEOF'
import os
from PIL import Image
img = Image.open(os.environ["ICON_SRC"]).convert("RGBA")
img.save(os.environ["ICNS_OUT"],
         sizes=[(16, 16), (32, 32), (64, 64), (128, 128),
                (256, 256), (512, 512), (1024, 1024)])
print("  ✓ AppIcon.icns written")
PYEOF
  else
    echo "  ⚠ No committed .icns and no Pillow fallback — bundle will lack its icon." >&2
  fi
fi

echo "  ✓ App bundle assembled at $APP_DIR"

# ── Package as DMG or ZIP ────────────────────────────────────────────────────
DMG_NAME="vadadee-berry-${VERSION}-macos-${ARCH}.dmg"
ZIP_NAME="vadadee-berry-${VERSION}-macos-${ARCH}.zip"
DMG_OUT="$DIST_DIR/$DMG_NAME"
ZIP_OUT="$DIST_DIR/$ZIP_NAME"
RELEASE_DIR="$ROOT/release"

make_zip() {
  echo "▶ Creating ZIP: $ZIP_NAME"
  (cd "$DIST_DIR" && rm -f "$ZIP_NAME" && zip -r -q --symlinks "$ZIP_NAME" "${APP_NAME}.app")
  echo "  ✓ $ZIP_OUT"
}

make_dmg_hdiutil() {
  # Native macOS: UDZO image with .app + Applications symlink.
  echo "▶ Creating DMG via hdiutil…"
  local staging
  staging="$(mktemp -d)"
  cp -r "$APP_DIR" "$staging/${APP_NAME}.app"
  ln -s /Applications "$staging/Applications"
  rm -f "$DMG_OUT"
  hdiutil create -volname "$APP_NAME" \
    -srcfolder "$staging" \
    -ov -format UDZO "$DMG_OUT" >/dev/null
  rm -rf "$staging"
  echo "  ✓ $DMG_OUT"
}

make_dmg_create_dmg() {
  echo "▶ Creating DMG via create-dmg…"
  rm -f "$DMG_OUT"
  create-dmg \
    --volname "$APP_NAME" \
    --window-pos 200 120 \
    --window-size 660 400 \
    --icon-size 128 \
    --icon "${APP_NAME}.app" 180 170 \
    --hide-extension "${APP_NAME}.app" \
    --app-drop-link 480 170 \
    "$DMG_OUT" \
    "$APP_DIR/.." 2>/dev/null || create-dmg \
    --volname "$APP_NAME" \
    --app-drop-link 480 170 \
    "$DMG_OUT" \
    "$APP_DIR/.."
  echo "  ✓ $DMG_OUT"
}

make_dmg_genisoimage() {
  echo "▶ Creating DMG via genisoimage…"
  rm -f "$DMG_OUT"
  local staging
  staging="$(mktemp -d)"
  cp -r "$APP_DIR" "$staging/${APP_NAME}.app"
  ln -s /Applications "$staging/Applications"
  local tool=genisoimage
  command -v genisoimage >/dev/null 2>&1 || tool=mkisofs
  # NOTE: -apple and -hfs are mutually exclusive in genisoimage; -hfsplus
  # alone still mounts on macOS (resource forks unused by this bundle).
  "$tool" -V "Vadadee Berry" \
    -D -r -hfsplus \
    -o "$DMG_OUT" \
    "$staging" 2>/dev/null
  rm -rf "$staging"
  echo "  ✓ $DMG_OUT"
}

mkdir -p "$DIST_DIR" "$RELEASE_DIR"
if [[ $FORCE_ZIP -eq 1 ]]; then
  make_zip
  FINAL="$ZIP_OUT"
elif [[ "$(uname -s)" == "Darwin" ]] && command -v hdiutil >/dev/null 2>&1; then
  make_dmg_hdiutil
  FINAL="$DMG_OUT"
elif command -v create-dmg &>/dev/null; then
  make_dmg_create_dmg
  FINAL="$DMG_OUT"
elif command -v genisoimage &>/dev/null || command -v mkisofs &>/dev/null; then
  make_dmg_genisoimage
  FINAL="$DMG_OUT"
else
  echo "  ℹ Neither hdiutil, create-dmg nor genisoimage found — ZIP fallback."
  make_zip
  FINAL="$ZIP_OUT"
fi

cp -f "$FINAL" "$RELEASE_DIR/"
BASENAME="$(basename "$FINAL")"
if command -v sha256sum >/dev/null 2>&1; then
  (cd "$RELEASE_DIR" && sha256sum "$BASENAME" > "${BASENAME}.sha256")
elif command -v shasum >/dev/null 2>&1; then
  (cd "$RELEASE_DIR" && shasum -a 256 "$BASENAME" > "${BASENAME}.sha256")
fi

echo
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo " Done! Distributable: $RELEASE_DIR/$BASENAME"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
