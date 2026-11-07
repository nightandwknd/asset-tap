#!/usr/bin/env bash
# Build the wasm32 harness and generate JS glue into web/www/pkg.
#   Needs: rust 1.97.x with the wasm32-unknown-unknown target, wasm-bindgen-cli ==
#   the `wasm-bindgen` version in Cargo.lock (printed below). No browser is started.
set -euo pipefail
cd "$(dirname "$0")/.."
WB_VER=$(awk '/^name = "wasm-bindgen"$/{getline; gsub(/"/,"",$3); print $3}' Cargo.lock)
command -v wasm-bindgen >/dev/null || { echo "install: cargo install wasm-bindgen-cli --version $WB_VER --locked"; exit 1; }
[ "$(wasm-bindgen --version | awk '{print $2}')" = "$WB_VER" ] || { echo "wasm-bindgen CLI must be $WB_VER"; exit 1; }
PROFILE=${PROFILE:-release}
cargo build -p browser-viewer-web --target wasm32-unknown-unknown $([ "$PROFILE" = release ] && echo --release)
OUT=${CARGO_TARGET_DIR:-target}/wasm32-unknown-unknown/$PROFILE/browser_viewer_web.wasm
wasm-bindgen --target web --no-typescript --out-dir web/www/pkg "$OUT"
ls -la web/www/pkg
