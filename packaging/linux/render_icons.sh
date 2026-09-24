#!/usr/bin/env bash
# =============================================================================
# render_icons.sh — icon set source of truth for Linux packaging.
#
# Source:  assets/icon_studio_1024.png (1024×1024 RGBA studio tile —
#          byte-identical to assets/vadadee_berry_icon.png, and the same left
#          tile of assets/logo.svg that the app uses for its window icon).
#          A PNG master is used deliberately: rasterizing logo.svg would need
#          its display fonts (CaskaydiaCove/Helvetica) and an SVG renderer,
#          making builds nondeterministic. PIL downscale is exact.
# Output:  packaging/linux/icons/hicolor/<size>x<size>/apps/
#          com.cosmobunny.VadadeeBerry.png   (16…512)
#
# The SAME files feed AppImage (AppDir icon tree), Flatpak (hicolor install),
# tarball (manual install) and the shared .desktop Icon= entry — one icon
# name everywhere, so docks/menus never fall back to a generic icon.
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SRC="$ROOT/assets/icon_studio_1024.png"
OUT_BASE="$ROOT/packaging/linux/icons/hicolor"
APP_ID="com.cosmobunny.VadadeeBerry"

if [[ ! -f "$SRC" ]]; then
  echo "ERROR: missing icon source: $SRC" >&2
  exit 1
fi

# shellcheck disable=SC2016
python3 - "$SRC" "$OUT_BASE" "$APP_ID" <<'PYEOF'
import sys
try:
    from PIL import Image
except ImportError:
    print("ERROR: Python Pillow is required (pip install pillow).", file=sys.stderr)
    sys.exit(1)
src, out_base, app_id = sys.argv[1], sys.argv[2], sys.argv[3]
master = Image.open(src).convert("RGBA")
assert master.size == (1024, 1024), f"unexpected master size {master.size}"

import os
for size in (16, 32, 48, 64, 128, 256, 512):
    d = os.path.join(out_base, f"{size}x{size}", "apps")
    os.makedirs(d, exist_ok=True)
    icon = master.resize((size, size), Image.LANCZOS)
    icon.save(os.path.join(d, f"{app_id}.png"))
    print(f"wrote {size}x{size}")
PYEOF

echo "Icon tree ready under $OUT_BASE"
