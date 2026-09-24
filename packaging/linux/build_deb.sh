#!/usr/bin/env bash
# =============================================================================
# build_deb.sh — Debian package for Vadadee Berry (studio + MCP companion).
#
# Layout (FHS, single app ID + icon everywhere):
#   /usr/bin/vadadee-berry, /usr/bin/vadadee-mcp-stdio
#   /usr/share/applications/com.cosmobunny.VadadeeBerry.desktop
#   /usr/share/icons/hicolor/<size>/apps/com.cosmobunny.VadadeeBerry.png
#   /usr/share/doc/vadadee-berry/{copyright,README}
#
# Depends: static list for Ubuntu 24.04 (t64 transition) with pre-t64
# alternates for older Debian/Ubuntu. CI install-tests the .deb
# (dpkg -i + apt-get -f + ldd check), which is the real enforcement —
# update this list if that step ever reports missing libraries.
#
# Usage:
#   ./packaging/linux/build_deb.sh [--bin path] [--mcp-bin path]
# Output:
#   release/vadadee-berry_<version>_<arch>.deb (+ .sha256)
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LINUX_DIR="$SCRIPT_DIR"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$ROOT"

APP_ID="com.cosmobunny.VadadeeBerry"
PKG="vadadee-berry"
BIN=""
MCP_BIN=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --bin) BIN="$2"; shift 2 ;;
    --mcp-bin) MCP_BIN="$2"; shift 2 ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "Unknown arg: $1" >&2; exit 1 ;;
  esac
done
[[ -z "$BIN" ]] && BIN="$ROOT/target/release/vadadee-berry"
[[ -z "$MCP_BIN" ]] && MCP_BIN="$ROOT/target/release/vadadee-mcp-stdio"

VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)"
VERSION="${VERSION:-0.0.2}"

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64) DEB_ARCH="amd64" ;;
  aarch64|arm64) DEB_ARCH="arm64" ;;
  *) echo "ERROR: unsupported arch for .deb: $ARCH" >&2; exit 1 ;;
esac

if ! command -v dpkg-deb >/dev/null 2>&1; then
  echo "ERROR: dpkg-deb not found (Debian/Ubuntu packaging tools required)." >&2
  exit 1
fi
for b in "$BIN" "$MCP_BIN"; do
  if [[ ! -x "$b" ]]; then
    echo "ERROR: binary not found/executable: $b" >&2
    exit 1
  fi
done

STAGE="$ROOT/dist/deb/${PKG}_${VERSION}_${DEB_ARCH}"
RELEASE_DIR="$ROOT/release"
OUT="$RELEASE_DIR/${PKG}_${VERSION}_${DEB_ARCH}.deb"

echo "▶ Staging $STAGE"
rm -rf "$STAGE"
mkdir -p "$STAGE/DEBIAN" \
  "$STAGE/usr/bin" \
  "$STAGE/usr/share/applications" \
  "$STAGE/usr/share/doc/$PKG"

cp "$BIN" "$STAGE/usr/bin/vadadee-berry"
cp "$MCP_BIN" "$STAGE/usr/bin/vadadee-mcp-stdio"
chmod 755 "$STAGE/usr/bin/vadadee-berry" "$STAGE/usr/bin/vadadee-mcp-stdio"
cp "$LINUX_DIR/$APP_ID.desktop" "$STAGE/usr/share/applications/$APP_ID.desktop"
mkdir -p "$STAGE/usr/share/icons"
cp -r "$LINUX_DIR/icons/hicolor" "$STAGE/usr/share/icons/hicolor"

cat > "$STAGE/usr/share/doc/$PKG/copyright" <<'EOF'
Vadadee Berry — vector, video and design editor.
License: MIT (see Cargo.toml / project LICENSE).
Application icon: studio tile derived from the project's own artwork.
EOF
cat > "$STAGE/DEBIAN/control" <<EOF
Package: $PKG
Version: $VERSION
Architecture: $DEB_ARCH
Maintainer: The Vadadee Berry contributors <https://github.com/CosmoBunny/vadadee-berry>
Section: graphics
Priority: optional
Depends: libasound2t64 | libasound2, libgtk-3-0t64 | libgtk-3-0, libx11-6, libxcb1, libxkbcommon0, libwayland-client0, libgl1, libvulkan1
Description: Vector, video and design editor
 Vadadee Berry is a vector, video and design editor: draw and edit vector
 artwork, animate it on a timeline, and export stills and video through one
 authoritative render pipeline. Ships the studio app plus the MCP stdio bridge.
EOF

cat > "$STAGE/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
update-desktop-database >/dev/null 2>&1 || true
gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor 2>/dev/null || true
EOF
chmod 755 "$STAGE/DEBIAN/postinst"

for s in 16 32 48 64 128 256 512; do
  f="$STAGE/usr/share/icons/hicolor/${s}x${s}/apps/$APP_ID.png"
  test -f "$f" || { echo "ERROR: missing icon $f" >&2; exit 1; }
done
if command -v desktop-file-validate >/dev/null 2>&1; then
  desktop-file-validate "$STAGE/usr/share/applications/$APP_ID.desktop"
fi

echo "▶ Building $OUT"
mkdir -p "$RELEASE_DIR"
rm -f "$OUT"
dpkg-deb --build --root-owner-group "$STAGE" "$OUT"

if command -v sha256sum >/dev/null 2>&1; then
  (cd "$RELEASE_DIR" && sha256sum "$(basename "$OUT")" > "$(basename "$OUT").sha256")
fi

echo " Done: $OUT"
dpkg-deb --info "$OUT" | head -8
