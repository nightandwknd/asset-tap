#!/usr/bin/env bash
# Package Asset Tap for Linux: .deb + AppImage + CLI archive
set -euo pipefail

echo "=== Packaging Linux .deb and AppImage ==="
(cd gui && cargo packager --release --formats deb,appimage)

# Inject CLI + fix desktop integration in .deb
# The .deb is in target/release/ after packaging
DEB_FILE=$(find target/release -name '*.deb' -print -quit 2>/dev/null || true)
if [ -n "$DEB_FILE" ]; then
  echo "=== Patching .deb package ==="
  WORK_DIR=$(mktemp -d)
  dpkg-deb -R "$DEB_FILE" "$WORK_DIR"

  # Inject CLI binary
  cp target/release/asset-tap "$WORK_DIR/usr/bin/"

  # Install app icon to standard hicolor theme (cargo-packager misses this)
  ICON_DIR="$WORK_DIR/usr/share/icons/hicolor/512x512/apps"
  mkdir -p "$ICON_DIR"
  cp assets/icon.png "$ICON_DIR/asset-tap-gui.png"
  echo "  -> Installed icon to hicolor/512x512/apps/"

  # Fix .desktop file: add StartupWMClass so the window manager
  # associates the running window with the launcher icon
  DESKTOP_FILE=$(find "$WORK_DIR" -name '*.desktop' -print -quit 2>/dev/null || true)
  if [ -n "$DESKTOP_FILE" ]; then
    if ! grep -q '^StartupWMClass=' "$DESKTOP_FILE"; then
      echo 'StartupWMClass=com.nightandwknd.asset-tap' >> "$DESKTOP_FILE"
    fi
    echo "  -> Patched $(basename "$DESKTOP_FILE") with StartupWMClass"
  fi

  dpkg-deb -b "$WORK_DIR" "$DEB_FILE"
  rm -rf "$WORK_DIR"
  echo "  -> $DEB_FILE (with CLI + desktop fix)"
fi

echo "=== Creating CLI archive ==="
mkdir -p cli-dist
cp target/release/asset-tap cli-dist/
(cd cli-dist && tar -czvf ../asset-tap-cli-linux.tar.gz *)
rm -rf cli-dist
echo "  -> asset-tap-cli-linux.tar.gz"
