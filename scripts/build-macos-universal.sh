#!/usr/bin/env bash
# Build universal macOS binaries (arm64 + x86_64), create .app bundle + DMG
set -euo pipefail

echo "=== Building for aarch64-apple-darwin ==="
cargo build --release --package asset-tap     --target aarch64-apple-darwin
cargo build --release --package asset-tap-gui --target aarch64-apple-darwin

echo "=== Building for x86_64-apple-darwin ==="
rustup target add x86_64-apple-darwin 2>/dev/null || true
cargo build --release --package asset-tap     --target x86_64-apple-darwin
cargo build --release --package asset-tap-gui --target x86_64-apple-darwin

echo "=== Creating universal binaries with lipo ==="
mkdir -p target/release
lipo -create -output target/release/asset-tap \
  target/aarch64-apple-darwin/release/asset-tap \
  target/x86_64-apple-darwin/release/asset-tap
lipo -create -output target/release/asset-tap-gui \
  target/aarch64-apple-darwin/release/asset-tap-gui \
  target/x86_64-apple-darwin/release/asset-tap-gui
file target/release/asset-tap
file target/release/asset-tap-gui

# Clean up per-architecture build artifacts to free disk space
echo "=== Cleaning up per-architecture targets ==="
rm -rf target/aarch64-apple-darwin target/x86_64-apple-darwin

echo "=== Packaging .app bundle ==="
(cd gui && cargo packager --release --formats app)

echo "=== Bundling CLI + creating DMG ==="
./scripts/package-macos.sh
