#!/usr/bin/env bash
# Package Asset Tap for macOS: .app bundle + CLI injection + DMG creation.
#
# If APPLE_SIGNING_IDENTITY is set, the .app and DMG are signed with the given
# Developer ID and (if APPLE_ID + APPLE_APP_PASSWORD + APPLE_TEAM_ID are set)
# notarized and stapled. Otherwise, falls back to ad-hoc signing for local dev.
set -euo pipefail

APP_DIR="target/release/Asset Tap.app"
DMG_OUT="target/release/AssetTap.dmg"
ENTITLEMENTS="gui/entitlements.plist"

if [ ! -d "$APP_DIR" ]; then
  echo "Error: No .app bundle found at $APP_DIR" >&2
  exit 1
fi

# Inject CLI binary into .app bundle
echo "Bundling CLI into .app..."
cp target/release/asset-tap "$APP_DIR/Contents/MacOS/"
echo "  -> $APP_DIR/Contents/MacOS/asset-tap"

if [ -n "${APPLE_SIGNING_IDENTITY:-}" ]; then
  echo "Signing .app with Developer ID: $APPLE_SIGNING_IDENTITY"
  # Sign inner executables (with entitlements) first, then the bundle itself.
  # Apple discourages --deep; instead we sign inside-out explicitly.
  # --options runtime enables hardened runtime (required for notarization).
  codesign --sign "$APPLE_SIGNING_IDENTITY" --force --timestamp --options runtime \
    --entitlements "$ENTITLEMENTS" \
    "$APP_DIR/Contents/MacOS/asset-tap" \
    "$APP_DIR/Contents/MacOS/asset-tap-gui"
  codesign --sign "$APPLE_SIGNING_IDENTITY" --force --timestamp --options runtime \
    "$APP_DIR"
  codesign --verify --verbose=2 --strict "$APP_DIR"
else
  echo "Signing .app bundle (ad-hoc; set APPLE_SIGNING_IDENTITY for official signing)..."
  codesign --sign - --force --deep "$APP_DIR"
fi

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

if [ -n "${APPLE_SIGNING_IDENTITY:-}" ]; then
  echo "Signing DMG..."
  codesign --sign "$APPLE_SIGNING_IDENTITY" --force --timestamp "$DMG_OUT"

  if [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_APP_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; then
    echo "Notarizing DMG (this can take a few minutes)..."
    # Capture submission output so we can fetch the log if notarization fails.
    SUBMIT_OUT=$(xcrun notarytool submit "$DMG_OUT" \
      --apple-id "$APPLE_ID" \
      --password "$APPLE_APP_PASSWORD" \
      --team-id "$APPLE_TEAM_ID" \
      --wait 2>&1) || {
      echo "$SUBMIT_OUT"
      SUBMISSION_ID=$(echo "$SUBMIT_OUT" | awk '/id:/ {print $2; exit}')
      if [ -n "$SUBMISSION_ID" ]; then
        echo "Notarization failed. Fetching log for submission $SUBMISSION_ID..."
        xcrun notarytool log "$SUBMISSION_ID" \
          --apple-id "$APPLE_ID" \
          --password "$APPLE_APP_PASSWORD" \
          --team-id "$APPLE_TEAM_ID" || true
      fi
      exit 1
    }
    echo "$SUBMIT_OUT"

    echo "Stapling notarization ticket..."
    xcrun stapler staple "$DMG_OUT"
    xcrun stapler validate "$DMG_OUT"

    echo "Verifying Gatekeeper acceptance..."
    spctl -a -vv -t install "$DMG_OUT"
  else
    echo "Skipping notarization (APPLE_ID / APPLE_APP_PASSWORD / APPLE_TEAM_ID not set)"
  fi
fi
