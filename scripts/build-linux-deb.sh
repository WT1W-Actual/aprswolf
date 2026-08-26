#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────
# APRSWolf — Linux (.deb) build for x86_64
# ─────────────────────────────────────────────────────────────
# Run this on an amd64 Linux machine/VM (Ubuntu/Debian-based).
#
# Prereqs handled automatically:
#   - Tauri v2 Linux system deps (GTK3, webkit2gtk, etc.)
#   - Rust `x86_64-unknown-linux-gnu` target
#   - Node dependencies
#
# Usage:
#   ./scripts/build-linux-deb.sh              build + verify + .deb
#   ./scripts/build-linux-deb.sh --no-tests   skip frontend/backend tests (faster)
#   ./scripts/build-linux-deb.sh --help       this message
# ─────────────────────────────────────────────────────────────

set -euo pipefail

# Allow overriding the Rust target being built.
TARGET="${RUST_LINUX_TARGET:-x86_64-unknown-linux-gnu}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_ROOT"

echo "═══ APRSWolf Linux .deb Build ═══  (target: ${TARGET})"
echo ""

RUN_TESTS=1
for arg in "$@"; do
  case "$arg" in
    --no-tests) RUN_TESTS=0 ;;
    --help|-h)
      sed -n '2,16p' "$BASH_SOURCE" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *) echo "Unknown option: $arg" >&2; exit 1 ;;
  esac
done

# Only allow amd64/arm64 Linux targets in this helper.
case "$TARGET" in
  x86_64-unknown-linux-gnu) ;;
  aarch64-unknown-linux-gnu) ;;
  *)
    echo "Unsupported target '$TARGET'. Use x86_64-unknown-linux-gnu or aarch64-unknown-linux-gnu." >&2
    exit 1
    ;;
esac

# ── 1. Install Tauri v2 Linux system dependencies ──
echo "── Installing Tauri Linux system dependencies ──"
if command -v apt-get >/dev/null 2>&1; then
  sudo apt-get update
  sudo apt-get install -y \
    libwebkit2gtk-4.1-dev \
    build-essential \
    curl \
    wget \
    file \
    libxdo-dev \
    libssl-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev
else
  echo "⚠  apt-get not found. Please install the Tauri Linux prerequisites for your distro:"
  echo "   https://v2.tauri.app/start/prerequisites/"
fi
echo ""

# ── 2. Ensure Rust target ──
echo "── Ensuring Rust toolchain (${TARGET}) ──"
if command -v rustup >/dev/null 2>&1; then
  rustup target add "$TARGET"
else
  echo "Rustup not found — installing Rust via rustup..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  source "$HOME/.cargo/env"
  rustup target add "$TARGET"
fi
echo ""

# ── 3. Node dependencies ──
echo "── Installing Node dependencies ──"
npm ci || npm install
echo ""

# ── 4. Tests / verification (optional) ──
if [[ "$RUN_TESTS" == "1" ]]; then
  echo "── Running tests ──"
  npm run verify || {
    echo "⚠  'npm run verify' failed. You may still build by using --no-tests." >&2
    exit 1
  }
  echo ""
fi

# ── 5. Build the .deb ──
echo "── Building Tauri bundle (.deb) for ${TARGET} ──"
npm run tauri build -- --bundles deb --target "$TARGET"
echo ""

# ── 6. Locate + rename the produced .deb to display-version form ──
TARGET_DIR="src-tauri/target/${TARGET}/release/bundle/deb"
echo "── Locating built .deb ──"
DEB=$(ls -t "$TARGET_DIR"/*.deb 2>/dev/null | head -1 || true)
if [[ -z "$DEB" ]]; then
  echo "⚠  No .deb found in $TARGET_DIR — check the build output above." >&2
  exit 1
fi

# Tauri names it like APRSWolf_1.0.0_amd64.deb . Mirror the macOS
# display-version naming: 1.0.0 -> 1.0 (keeps the .0), so the .deb is
# APRSWolf_1.0_amd64.deb.
DISPLAY_PKG_VERSION=$(node -e '
const p = require("./package.json");
const v = p.displayVersion || p.version;
const m = String(v).match(/^(\d+)\.(\d+)\.(\d+)$/);
if (m) process.stdout.write(`${Number(m[1])}.${Number(m[2])}`);
else process.stdout.write(String(v).replace(/\b\d+\b/g, (s) => String(Number(s))));
')

ARCH="${TARGET%%-*}"
case "$ARCH" in
  x86_64) DEB_ARCH="amd64" ;;
  aarch64) DEB_ARCH="arm64" ;;
esac

FINAL_NAME="APRSWolf_${DISPLAY_PKG_VERSION}_${DEB_ARCH}.deb"
OUT_DIR="$PROJECT_ROOT/dist-linux"
mkdir -p "$OUT_DIR"

FINAL_PATH="$OUT_DIR/$FINAL_NAME"
rm -f "$FINAL_PATH"
cp "$DEB" "$FINAL_PATH"

SIZE=$(du -h "$FINAL_PATH" | cut -f1)
echo ""
echo "═══ Done: ${FINAL_PATH} (${SIZE}) ═══"
echo ""
echo "Bundle output is also at:"
echo "  $TARGET_DIR/$(basename "$DEB")"
