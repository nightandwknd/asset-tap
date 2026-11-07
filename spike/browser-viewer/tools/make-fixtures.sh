#!/usr/bin/env bash
# Regenerate every fixture from scratch: synthetic mannequin -> real rig bind+bake (native, 1 thread).
#   Needs: python3, the repo's packs/ual1 (CC0 Quaternius animation), cargo.
set -euo pipefail
cd "$(dirname "$0")/.."
REPO=$(cd ../.. && pwd)
mkdir -p web/www/fixtures; T=$(mktemp -d)
cargo build --release -p native-probe
BIN=${CARGO_TARGET_DIR:-target}/release/native-probe
for g in 4 20 55; do
  python3 -I tools/gen_fixture.py "$T/m$g.glb" --grid $g
  RAYON_NUM_THREADS=1 "$BIN" "$T/m$g.glb" "$REPO/packs/ual1" "$T/o$g" >/dev/null
  cp "$T/o$g/baked.glb" web/www/fixtures/mannequin_g${g}_baked.glb
  [ $g = 4 ] && cp "$T/m$g.glb" web/www/fixtures/mannequin_g4.glb
done
ls -la web/www/fixtures
