#!/usr/bin/env bash
# =============================================================================
# make_macos_installer.sh
# Assembles "Vadadee Berry.app" from cross-compiled binaries and packages it
# into a distributable .dmg (or .zip as fallback).
#
# Usage:
#   ./packaging/macos/make_macos_installer.sh [--arch aarch64|x86_64] [--zip]
#
# Requirements (on the host Linux machine):
#   - Built binaries in target/<arch>-apple-darwin/release/
#   - python3  (for icns generation via png2icns helper)
#   - libguestfs / genisoimage  OR  create-dmg (for DMG)
#   - Alternatively the krama-mac-builder Docker image (used automatically)
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$SCRIPT_DIR/../.."

# ── Defaults ─────────────────────────────────────────────────────────────────
ARCH="aarch64"
FORCE_ZIP=0
VERSION="0.1.0"
APP_NAME="Vadadee Berry"
BUNDLE_ID="com.vadadee.berry"
DIST_DIR="$ROOT/dist/macos"

# ── Parse args ────────────────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
  case "$1" in
    --arch) ARCH="$2"; shift 2 ;;
    --zip)  FORCE_ZIP=1; shift ;;
    *) echo "Unknown arg: $1"; exit 1 ;;
  esac
done

TARGET="${ARCH}-apple-darwin"
BIN_DIR="$ROOT/target/$TARGET/release"
RELEASE_DIR="$DIST_DIR/${APP_NAME}.app"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo " Building: $APP_NAME v$VERSION"
echo " Arch    : $ARCH  ($TARGET)"
echo " Bins    : $BIN_DIR"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# ── Sanity check binaries ─────────────────────────────────────────────────────
for bin in vadadee-berry vadadee-mcp-stdio; do
  if [[ ! -f "$BIN_DIR/$bin" ]]; then
    echo "✗ Missing binary: $BIN_DIR/$bin"
    echo "  Run: sg docker -c \"docker run --rm -v \$PWD:/io -w /io \\"
    echo "         -e SDKROOT=/opt/MacOSX11.3.sdk -e MACOSX_DEPLOYMENT_TARGET=11.3 \\"
    echo "         -e RUSTFLAGS='-C link-arg=-undefined -C link-arg=dynamic_lookup' \\"
    echo "         krama-mac-builder:latest \\"
    echo "         cargo zigbuild --target $TARGET --release --bin vadadee-berry --bin vadadee-mcp-stdio\""
    exit 1
  fi
done

# ── Create .app skeleton ──────────────────────────────────────────────────────
echo
echo "▶ Assembling .app bundle…"
rm -rf "$RELEASE_DIR"
mkdir -p "$RELEASE_DIR/Contents/MacOS"
mkdir -p "$RELEASE_DIR/Contents/Resources"

# Copy binaries
cp "$BIN_DIR/vadadee-berry"     "$RELEASE_DIR/Contents/MacOS/vadadee-berry"
cp "$BIN_DIR/vadadee-mcp-stdio" "$RELEASE_DIR/Contents/MacOS/vadadee-mcp-stdio"
chmod +x "$RELEASE_DIR/Contents/MacOS/vadadee-berry"
chmod +x "$RELEASE_DIR/Contents/MacOS/vadadee-mcp-stdio"

# Copy Info.plist
cp "$SCRIPT_DIR/Info.plist" "$RELEASE_DIR/Contents/Info.plist"

# Write PkgInfo (required by macOS)
echo -n "APPL????" > "$RELEASE_DIR/Contents/PkgInfo"

# ── Generate .icns ─────────────────────────────────────────────────────────────
ICON_SRC="$ROOT/vadadee_berry_icon.png"
ICNS_OUT="$RELEASE_DIR/Contents/Resources/AppIcon.icns"

if [[ -f "$ICON_SRC" ]]; then
  echo "▶ Generating AppIcon.icns from $ICON_SRC…"
  python3 - <<'PYEOF'
import struct, zlib, os, sys
from pathlib import Path

src = Path(os.environ.get("ICON_SRC", ""))
out = Path(os.environ.get("ICNS_OUT", ""))

try:
    from PIL import Image
except ImportError:
    print("  ⚠ Pillow not found — icon will be skipped. Install with: pip install Pillow")
    sys.exit(0)

img = Image.open(src).convert("RGBA")

# ICNS size codes
sizes = [16, 32, 64, 128, 256, 512, 1024]
code_map = {
    16:   (b'icp4', b'icp5'),   # (1x, 2x)
    32:   (b'icp5', b'icp6'),
    64:   (b'icp6', None),
    128:  (b'ic07', b'ic08'),
    256:  (b'ic08', b'ic09'),
    512:  (b'ic09', b'ic10'),
    1024: (b'ic10', None),
}

import io as _io

chunks = []
for sz, (code1x, code2x) in code_map.items():
    scaled = img.resize((sz, sz), Image.LANCZOS)
    buf = _io.BytesIO()
    scaled.save(buf, format="PNG")
    data = buf.getvalue()
    chunks.append((code1x, data))

total = 8 + sum(8 + len(d) for _, d in chunks)
with open(out, "wb") as f:
    f.write(b"icns")
    f.write(struct.pack(">I", total))
    for code, data in chunks:
        f.write(code)
        f.write(struct.pack(">I", 8 + len(data)))
        f.write(data)

print(f"  ✓ Written {out} ({total} bytes, {len(chunks)} sizes)")
PYEOF
  ICON_SRC="$ICON_SRC" ICNS_OUT="$ICNS_OUT" python3 - <<'PYEOF'
import struct, os, sys, io as _io
from pathlib import Path

src  = Path(os.environ["ICON_SRC"])
out  = Path(os.environ["ICNS_OUT"])

try:
    from PIL import Image
except ImportError:
    print("  ⚠ Pillow not found — skipping icon. Install: pip install Pillow")
    sys.exit(0)

img = Image.open(src).convert("RGBA")

sizes = [16, 32, 64, 128, 256, 512, 1024]
codes = [b'icp4', b'icp5', b'icp6', b'ic07', b'ic08', b'ic09', b'ic10']

chunks = []
for sz, code in zip(sizes, codes):
    scaled = img.resize((sz, sz), Image.LANCZOS)
    buf = _io.BytesIO()
    scaled.save(buf, format="PNG")
    chunks.append((code, buf.getvalue()))

total = 8 + sum(8 + len(d) for _, d in chunks)
out.parent.mkdir(parents=True, exist_ok=True)
with open(out, "wb") as f:
    f.write(b"icns")
    f.write(struct.pack(">I", total))
    for code, data in chunks:
        f.write(code)
        f.write(struct.pack(">I", 8 + len(data)))
        f.write(data)
print(f"  ✓ {out.name}  ({total} bytes)")
PYEOF
else
  echo "  ⚠ No icon found at $ICON_SRC — skipping .icns"
fi

echo "  ✓ App bundle assembled at $RELEASE_DIR"

# ── Package as DMG or ZIP ─────────────────────────────────────────────────────
DMG_NAME="${APP_NAME// /_}_${VERSION}_macOS_${ARCH}.dmg"
ZIP_NAME="${APP_NAME// /_}_${VERSION}_macOS_${ARCH}.zip"
DMG_OUT="$DIST_DIR/$DMG_NAME"
ZIP_OUT="$DIST_DIR/$ZIP_NAME"

make_zip() {
  echo "▶ Creating ZIP: $ZIP_NAME"
  cd "$DIST_DIR"
  zip -r --symlinks "$ZIP_NAME" "${APP_NAME}.app"
  echo "  ✓ $ZIP_OUT"
}

make_dmg_genisoimage() {
  # Create a sparse directory layout then use genisoimage/mkisofs HFS+
  echo "▶ Creating DMG via genisoimage…"
  local staging
  staging=$(mktemp -d)
  cp -r "$RELEASE_DIR" "$staging/${APP_NAME}.app"
  ln -s /Applications "$staging/Applications"
  genisoimage -V "Vadadee Berry" \
    -D -r -apple -hfs \
    -o "$DMG_OUT" \
    "$staging" 2>/dev/null
  rm -rf "$staging"
  echo "  ✓ $DMG_OUT"
}

make_dmg_create_dmg() {
  echo "▶ Creating DMG via create-dmg…"
  create-dmg \
    --volname "$APP_NAME" \
    --volicon "$RELEASE_DIR/Contents/Resources/AppIcon.icns" \
    --window-pos 200 120 \
    --window-size 660 400 \
    --icon-size 128 \
    --icon "${APP_NAME}.app" 180 170 \
    --hide-extension "${APP_NAME}.app" \
    --app-drop-link 480 170 \
    "$DMG_OUT" \
    "$DIST_DIR"
  echo "  ✓ $DMG_OUT"
}

if [[ $FORCE_ZIP -eq 1 ]]; then
  make_zip
elif command -v create-dmg &>/dev/null; then
  make_dmg_create_dmg
elif command -v genisoimage &>/dev/null || command -v mkisofs &>/dev/null; then
  make_dmg_genisoimage
else
  echo "  ℹ  Neither create-dmg nor genisoimage found — falling back to ZIP."
  echo "     To get a proper DMG:  sudo apt install genisoimage"
  echo "     Or:                   brew install create-dmg  (on macOS)"
  make_zip
fi

echo
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo " Done! Distributable is in: $DIST_DIR"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
