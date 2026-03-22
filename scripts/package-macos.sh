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

# Create compressed DMG (two-step to avoid hdiutil sizing issues)
echo "Creating DMG..."
TMP_DMG="target/release/AssetTap_tmp.dmg"
rm -f "$DMG_OUT" "$TMP_DMG"
hdiutil create -volname "Asset Tap" -srcfolder "$APP_DIR" \
  -ov -format UDRW -fs HFS+ -megabytes 512 "$TMP_DMG"
hdiutil convert "$TMP_DMG" -format UDZO -o "$DMG_OUT"
rm -f "$TMP_DMG"
echo "  -> $DMG_OUT"
