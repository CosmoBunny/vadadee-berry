#!/usr/bin/env bash
# =============================================================================
# make_desktop_release.sh
# Compile host desktop release binaries, bundle them, and copy the archive into
# the main project `release/` folder (for user downloads).
#
# Usage:
#   ./packaging/make_desktop_release.sh
#   ./packaging/make_desktop_release.sh --no-build          # package existing bins
#   ./packaging/make_desktop_release.sh --bins studio       # only vadadee-berry
#   ./packaging/make_desktop_release.sh --bins all          # studio + mcp (default)
#   ./packaging/make_desktop_release.sh --features ""       # cargo --no-default-features
#   OPENCV=0 ./packaging/make_desktop_release.sh            # same: no opencv feature
#
# Output (example on Linux x86_64):
#   dist/desktop/vadadee-berry-0.1.0-linux-x86_64/
#   dist/desktop/vadadee-berry-0.1.0-linux-x86_64.tar.gz
#   release/vadadee-berry-0.1.0-linux-x86_64.tar.gz
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT"

VERSION="$(
  sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1
)"
VERSION="${VERSION:-0.1.0}"

DO_BUILD=1
BINS="all"
# Extra cargo flags (array via string is fragile; keep simple)
CARGO_EXTRA=()
NO_DEFAULT=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --no-build) DO_BUILD=0; shift ;;
    --bins) BINS="$2"; shift 2 ;;
    --no-default-features) NO_DEFAULT=1; shift ;;
    --features)
      CARGO_EXTRA+=(--features "$2"); shift 2 ;;
    -h|--help)
      sed -n '2,25p' "$0"; exit 0 ;;
    *)
      echo "Unknown arg: $1" >&2
      exit 1
      ;;
  esac
done

if [[ "${OPENCV:-1}" == "0" ]]; then
  NO_DEFAULT=1
fi

OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64) ARCH_TAG="x86_64" ;;
  aarch64|arm64) ARCH_TAG="aarch64" ;;
  *) ARCH_TAG="$ARCH" ;;
esac

case "$OS" in
  linux)  PLATFORM="linux" ;;
  darwin) PLATFORM="macos" ;;
  mingw*|msys*|cygwin*|windows*) PLATFORM="windows" ;;
  *) PLATFORM="$OS" ;;
esac

BUNDLE_NAME="vadadee-berry-${VERSION}-${PLATFORM}-${ARCH_TAG}"
DIST_DIR="$ROOT/dist/desktop"
BUNDLE_DIR="$DIST_DIR/$BUNDLE_NAME"
ARCHIVE_TGZ="$DIST_DIR/${BUNDLE_NAME}.tar.gz"
ARCHIVE_ZIP="$DIST_DIR/${BUNDLE_NAME}.zip"
RELEASE_DIR="$ROOT/release"

BIN_STUDIO="vadadee-berry"
BIN_MCP="vadadee-mcp-stdio"
if [[ "$PLATFORM" == "windows" ]]; then
  BIN_STUDIO="vadadee-berry.exe"
  BIN_MCP="vadadee-mcp-stdio.exe"
fi

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo " Vadadee Berry desktop release"
echo " Version : $VERSION"
echo " Host    : $PLATFORM / $ARCH_TAG"
echo " Bundle  : $BUNDLE_NAME"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# ── Build ────────────────────────────────────────────────────────────────────
if [[ $DO_BUILD -eq 1 ]]; then
  echo
  echo "▶ cargo build --release …"
  BUILD_ARGS=(build --release)
  if [[ $NO_DEFAULT -eq 1 ]]; then
    BUILD_ARGS+=(--no-default-features)
  fi
  if [[ ${#CARGO_EXTRA[@]} -gt 0 ]]; then
    BUILD_ARGS+=("${CARGO_EXTRA[@]}")
  fi
  case "$BINS" in
    studio) BUILD_ARGS+=(--bin vadadee-berry) ;;
    mcp)    BUILD_ARGS+=(--bin vadadee-mcp-stdio) ;;
    all|*)  BUILD_ARGS+=(--bin vadadee-berry --bin vadadee-mcp-stdio) ;;
  esac
  cargo "${BUILD_ARGS[@]}"
else
  echo "▶ Skipping build (--no-build)"
fi

REL_BIN_DIR="$ROOT/target/release"
need_bin() {
  local b="$1"
  if [[ ! -f "$REL_BIN_DIR/$b" ]]; then
    echo "✗ Missing binary: $REL_BIN_DIR/$b" >&2
    echo "  Run without --no-build, or build it first." >&2
    exit 1
  fi
}

case "$BINS" in
  studio) need_bin "$BIN_STUDIO" ;;
  mcp)    need_bin "$BIN_MCP" ;;
  all|*)  need_bin "$BIN_STUDIO"; need_bin "$BIN_MCP" ;;
esac

# ── Stage bundle ─────────────────────────────────────────────────────────────
echo
echo "▶ Staging bundle at $BUNDLE_DIR"
rm -rf "$BUNDLE_DIR"
mkdir -p "$BUNDLE_DIR/bin" "$BUNDLE_DIR/assets"

copy_bin() {
  local name="$1"
  cp "$REL_BIN_DIR/$name" "$BUNDLE_DIR/bin/$name"
  chmod +x "$BUNDLE_DIR/bin/$name" 2>/dev/null || true
}

case "$BINS" in
  studio) copy_bin "$BIN_STUDIO" ;;
  mcp)    copy_bin "$BIN_MCP" ;;
  all|*)  copy_bin "$BIN_STUDIO"; copy_bin "$BIN_MCP" ;;
esac

# Logos: full sheet + per-product tiles (studio left, mcp right of logo.svg)
for f in logo.svg logo.png icon_studio.png icon_mcp.png icon_studio_1024.png; do
  if [[ -f "$ROOT/assets/$f" ]]; then
    cp "$ROOT/assets/$f" "$BUNDLE_DIR/assets/$f"
  fi
done

# Desktop entry (Linux)
if [[ "$PLATFORM" == "linux" ]]; then
  cat > "$BUNDLE_DIR/vadadee-berry.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Vadadee Berry
Comment=Vector / video / shader creative studio
Exec=bin/${BIN_STUDIO}
Icon=assets/icon_studio.png
Terminal=false
Categories=Graphics;AudioVideo;
EOF
fi

cat > "$BUNDLE_DIR/README.txt" <<EOF
Vadadee Berry ${VERSION} — ${PLATFORM} ${ARCH_TAG}
========================================

Binaries
  bin/${BIN_STUDIO}       Studio desktop app (window icon: studio tile)
  bin/${BIN_MCP}   MCP stdio bridge (icon: assets/icon_mcp.png)

Logos (from assets/logo.svg, 200×100)
  Left  100×100  → studio  (icon_studio.png)
  Right 100×100  → MCP     (icon_mcp.png)

Run (from this directory)
  ./bin/${BIN_STUDIO}

Video import/export needs FFmpeg shared libraries at runtime (optional).
OpenCV is optional at build time; this build may or may not include it.

See project README for full platform notes.
EOF

# ── Archive ──────────────────────────────────────────────────────────────────
echo "▶ Creating archive …"
mkdir -p "$DIST_DIR" "$RELEASE_DIR"
FINAL_ARCHIVE=""

if [[ "$PLATFORM" == "windows" ]] && command -v zip >/dev/null 2>&1; then
  rm -f "$ARCHIVE_ZIP"
  (cd "$DIST_DIR" && zip -r -q "$(basename "$ARCHIVE_ZIP")" "$BUNDLE_NAME")
  FINAL_ARCHIVE="$ARCHIVE_ZIP"
else
  tar -C "$DIST_DIR" -czf "$ARCHIVE_TGZ" "$BUNDLE_NAME"
  FINAL_ARCHIVE="$ARCHIVE_TGZ"
fi

if [[ -z "${FINAL_ARCHIVE}" || ! -f "$FINAL_ARCHIVE" ]]; then
  echo "✗ Failed to create archive" >&2
  exit 1
fi

# ── Copy into main project release/ (download drop folder) ───────────────────
cp -f "$FINAL_ARCHIVE" "$RELEASE_DIR/"
BASENAME="$(basename "$FINAL_ARCHIVE")"

# SHA256 for integrity (handy next to the download)
if command -v sha256sum >/dev/null 2>&1; then
  (cd "$RELEASE_DIR" && sha256sum "$BASENAME" > "${BASENAME}.sha256")
elif command -v shasum >/dev/null 2>&1; then
  (cd "$RELEASE_DIR" && shasum -a 256 "$BASENAME" > "${BASENAME}.sha256")
fi

# Keep a stable "latest" pointer (symlink or copy)
LATEST_EXT="${BASENAME##*.}"
if [[ "$BASENAME" == *.tar.gz ]]; then
  LATEST_EXT="tar.gz"
elif [[ "$BASENAME" == *.zip ]]; then
  LATEST_EXT="zip"
fi
LATEST_LINK="$RELEASE_DIR/vadadee-berry-latest-${PLATFORM}-${ARCH_TAG}.${LATEST_EXT}"
rm -f "$RELEASE_DIR/vadadee-berry-latest-${PLATFORM}-${ARCH_TAG}".* 2>/dev/null || true
ln -s "$BASENAME" "$LATEST_LINK" 2>/dev/null || cp -f "$RELEASE_DIR/$BASENAME" "$LATEST_LINK"

echo
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo " Done"
echo " Bundle dir : $BUNDLE_DIR"
echo " Archive    : $FINAL_ARCHIVE"
echo " Download   : $RELEASE_DIR/$BASENAME"
echo " Latest     : $LATEST_LINK"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo
echo "Publish: host the files under release/ for user download."
