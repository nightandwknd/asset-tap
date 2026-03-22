#!/usr/bin/env bash
# Package Asset Tap for Windows: NSIS installer + CLI archive
set -euo pipefail

echo "=== Packaging Windows NSIS installer ==="
(cd gui && cargo packager --release --formats nsis)

echo "=== Creating CLI archive ==="
mkdir -p cli-dist
cp target/release/asset-tap.exe cli-dist/
(cd cli-dist && 7z a -tzip ../asset-tap-cli-windows.zip *)
rm -rf cli-dist
echo "  -> asset-tap-cli-windows.zip"
