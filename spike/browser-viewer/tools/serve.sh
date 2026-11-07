#!/usr/bin/env bash
# Static server for the harness, localhost only. Prints the URL; Ctrl-C to stop.
cd "$(dirname "$0")/../web/www"
PORT=${PORT:-8137}
echo "serving http://127.0.0.1:$PORT/  (fixtures: ?glb=fixtures/mannequin_g4_baked.glb)"
exec python3 -m http.server "$PORT" --bind 127.0.0.1
