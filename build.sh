#!/usr/bin/env zsh
# ─────────────────────────────────────────────────────────────
# APRSWolf — Full build + universal DMG
# ─────────────────────────────────────────────────────────────
# Usage:
#   ./build.sh                  build universal DMG
#   ./build.sh --no-clean       skip cargo clean (faster rebuild)
#   ./build.sh --help           this message
# ─────────────────────────────────────────────────────────────

set -e

SCRIPT_DIR="${0:A:h}"
cd "$SCRIPT_DIR"

echo "═══ APRSWolf Full Build ═══"
echo ""

# ── 1. Frontend: type-check + vite build ──
echo "── Frontend build ──"
npm run build
echo ""

# ── 2. Tests ──
echo "── Tests ──"
npx vitest run
echo ""

# ── 3. Universal DMG ──
if [[ "$1" == "--no-clean" ]]; then
  echo "── Universal DMG (skipping cargo clean) ──"
  node scripts/build-macos-universal-no-clean.cjs 2>/dev/null || {
    echo "⚠  no-clean script not found, running standard build..."
    node scripts/build-macos-universal.cjs
  }
else
  echo "── Universal DMG ──"
  node scripts/build-macos-universal.cjs
fi
echo ""

# ── 4. Show result ──
DMG=$(ls -t src-tauri/target/universal-apple-darwin/release/bundle/dmg/*.dmg 2>/dev/null | head -1)
if [[ -n "$DMG" ]]; then
  SIZE=$(stat -f "%z" "$DMG" | awk '{printf "%.1f MB", $1/1048576}')
  echo "═══ Done: ${DMG:t} (${SIZE}) ═══"
else
  echo "═══ Done (DMG not found — check for errors above) ═══"
fi
echo ""

# ── 5. Copy Apple Silicon .app to /Applications ──
# Prefer the aarch64 (Apple Silicon) .app bundle; fall back to universal.
APP_SRC=""
if [[ -d "src-tauri/target/aarch64-apple-darwin/release/bundle/macos/APRSWolf.app" ]]; then
  APP_SRC="src-tauri/target/aarch64-apple-darwin/release/bundle/macos/APRSWolf.app"
elif [[ -d "src-tauri/target/universal-apple-darwin/release/bundle/macos/APRSWolf.app" ]]; then
  APP_SRC="src-tauri/target/universal-apple-darwin/release/bundle/macos/APRSWolf.app"
fi

if [[ -n "$APP_SRC" ]]; then
  DEST="/Applications/APRSWolf.app"
  echo "── Installing to ${DEST} ──"

  # Remove old version if present
  if [[ -e "$DEST" ]]; then
    rm -rf "$DEST"
  fi

  # Copy (handles both file and .app bundle)
  cp -R "$APP_SRC" "$DEST"
  echo "═══ Installed: ${DEST} ═══"
else
  echo "⚠  APRSWolf binary not found — skipping install"
fi
