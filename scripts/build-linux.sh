#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────
# APRSWolf — Linux build (.deb + .rpm + .AppImage)
# ─────────────────────────────────────────────────────────────
# Run this on a Linux machine/VM (Ubuntu/Debian-based recommended).
#
# Prereqs handled automatically:
#   - Tauri v2 Linux system deps (GTK3, webkit2gtk, etc.)
#   - rpmbuild (for the .rpm bundle)
#   - Rust target (x86_64 or aarch64)
#   - Node dependencies
#
# Usage:
#   ./scripts/build-linux.sh              build + verify + all bundles
#   ./scripts/build-linux.sh --no-tests   skip frontend/backend tests (faster)
#   ./scripts/build-linux.sh --bundles deb,rpm,appimage
#   ./scripts/build-linux.sh --help       this message
#
# Environment:
#   RUST_LINUX_TARGET   override the Rust target
#                       (default: x86_64-unknown-linux-gnu)
# ─────────────────────────────────────────────────────────────

set -euo pipefail

# Allow overriding the Rust target being built.
TARGET="${RUST_LINUX_TARGET:-x86_64-unknown-linux-gnu}"
BUNDLES="deb,rpm,appimage"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_ROOT"

echo "═══ APRSWolf Linux Build ═══  (target: ${TARGET})"
echo ""

RUN_TESTS=1
for arg in "$@"; do
  case "$arg" in
    --no-tests) RUN_TESTS=0 ;;
    --bundles=*) BUNDLES="${arg#--bundles=}" ;;
    --bundles)
      echo "Use --bundles=deb,rpm,appimage (with '=')." >&2
      exit 1
      ;;
    --help|-h)
      sed -n '2,22p' "$BASH_SOURCE" | sed 's/^# \{0,1\}//'
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
    librsvg2-dev \
    rpm \
    xdg-utils
else
  echo "⚠  apt-get not found. Please install the Tauri Linux prerequisites for your distro:"
  echo "   https://v2.tauri.app/start/prerequisites/"
  echo "   (The .rpm bundle additionally requires 'rpmbuild' from the 'rpm' package.)"
  echo "   (The .AppImage bundle additionally requires 'xdg-open' from the 'xdg-utils' package.)"
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

# ── 5. Build the bundles ──
echo "── Building Tauri bundles (${BUNDLES}) for ${TARGET} ──"
npm run tauri build -- --bundles "$BUNDLES" --target "$TARGET"
echo ""

# ── 6. Locate + rename produced bundles to display-version form ──
BUNDLE_DIR="src-tauri/target/${TARGET}/release/bundle"
OUT_DIR="$PROJECT_ROOT/dist-linux"
mkdir -p "$OUT_DIR"

# Artifacts carry the full package.json version, e.g. APRSWolf_1.0.3_amd64.deb.
PKG_VERSION=$(node -p 'require("./package.json").version')

ARCH="${TARGET%%-*}"
case "$ARCH" in
  x86_64) PKG_ARCH="amd64" ;;
  aarch64) PKG_ARCH="arm64" ;;
esac

copy_bundle() {
  local src="$1" final_name="$2"
  if [[ -z "$src" || ! -f "$src" ]]; then
    echo "⚠  Expected bundle not found for ${final_name}" >&2
    return 1
  fi
  local final_path="$OUT_DIR/$final_name"
  rm -f "$final_path"
  cp "$src" "$final_path"
  echo "  → ${final_path} ($(du -h "$final_path" | cut -f1))"
}

echo "── Collecting bundles into dist-linux/ ──"

# .deb
DEB=$(ls -t "$BUNDLE_DIR/deb"/*.deb 2>/dev/null | head -1 || true)
copy_bundle "$DEB" "APRSWolf_${PKG_VERSION}_${PKG_ARCH}.deb" || true

# .rpm
RPM=$(ls -t "$BUNDLE_DIR/rpm"/*.rpm 2>/dev/null | head -1 || true)
copy_bundle "$RPM" "APRSWolf-${PKG_VERSION}-1.${PKG_ARCH}.rpm" || true

# .AppImage
APPIMAGE=$(ls -t "$BUNDLE_DIR/appimage"/*.AppImage 2>/dev/null | head -1 || true)
copy_bundle "$APPIMAGE" "APRSWolf_${PKG_VERSION}_${PKG_ARCH}.AppImage" || true

echo ""
echo "═══ Done ═══"
echo "Artifacts in: $OUT_DIR"
ls -1 "$OUT_DIR" 2>/dev/null || true
