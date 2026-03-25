#!/usr/bin/env bash
# Package Asset Tap for macOS: .app bundle + CLI injection + DMG creation
set -euo pipefail

APP_DIR="target/release/Asset Tap.app"
DMG_OUT="target/release/AssetTap.dmg"

if [ -z "$APP_DIR" ]; then
  echo "Error: No .app bundle found in target/release/" >&2
  exit 1
fi

# Inject CLI binary into .app bundle
echo "Bundling CLI into .app..."
cp target/release/asset-tap "$APP_DIR/Contents/MacOS/"
echo "  -> $APP_DIR/Contents/MacOS/asset-tap"

# Re-sign bundle after CLI injection
echo "Signing .app bundle (ad-hoc)..."
codesign --sign - --force --deep "$APP_DIR"

# Create compressed DMG
echo "Creating DMG..."
rm -f "$DMG_OUT"

# Detach any leftover "Asset Tap" volume from a previous run
if hdiutil info 2>/dev/null | grep -q "/Volumes/Asset Tap"; then
  echo "Detaching stale /Volumes/Asset Tap..."
  hdiutil detach "/Volumes/Asset Tap" -force 2>/dev/null || true
fi

hdiutil create -volname "Asset Tap" -srcfolder "$APP_DIR" \
  -ov -format UDZO -fs HFS+ "$DMG_OUT"
echo "  -> $DMG_OUT"
