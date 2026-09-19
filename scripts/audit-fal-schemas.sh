#!/usr/bin/env bash
# Audit providers/fal-ai.yaml against fal's live per-endpoint OpenAPI schemas.
#
# On demand only (needs network). Prints, per model: keys we send that the
# schema doesn't know, schema properties we neither send nor expose, declared
# parameters whose enum/default/min/max/type disagree with the schema, and
# required fields we omit. Exits non-zero when any finding remains after the
# allowlist is applied, so it can be used as a gate.
#
# Usage: scripts/audit-fal-schemas.sh [--json] [--cache-dir DIR] [--refresh]
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
YAML="$REPO_ROOT/providers/fal-ai.yaml"
PROVIDER_ID="fal.ai"
ALLOWLIST="$REPO_ROOT/scripts/audit-fal-allowlist.json"
CACHE_DIR="${TMPDIR:-/tmp}/asset-tap-fal-schemas"
SCHEMA_URL_BASE='https://fal.ai/api/openapi/queue/openapi.json?endpoint_id='
JSON_OUT=0
REFRESH=0

while [ $# -gt 0 ]; do
    case "$1" in
        --json) JSON_OUT=1 ;;
        --refresh) REFRESH=1 ;;
        --cache-dir)
            shift
            [ $# -gt 0 ] || { echo "--cache-dir needs a directory" >&2; exit 2; }
            CACHE_DIR="$1"
            ;;
        -h | --help)
            sed -n '2,12p' "${BASH_SOURCE[0]}"
            exit 0
            ;;
        *)
            echo "unknown argument: $1" >&2
            exit 2
            ;;
    esac
    shift
done

command -v curl > /dev/null || { echo "curl not found" >&2; exit 2; }
command -v python3 > /dev/null || { echo "python3 not found" >&2; exit 2; }
command -v cargo > /dev/null || { echo "cargo not found" >&2; exit 2; }
[ -f "$YAML" ] || { echo "missing $YAML" >&2; exit 2; }
[ -f "$ALLOWLIST" ] || { echo "missing $ALLOWLIST" >&2; exit 2; }

mkdir -p "$CACHE_DIR"

# Read the config through the app's own parser rather than a second, hand-rolled
# YAML reader: --dump-provider-config prints the resolved ProviderConfig (anchors
# expanded) as JSON, so the audit can never disagree with what the app loads.
CONFIG_JSON="$CACHE_DIR/$PROVIDER_ID.config.json"
[ "$JSON_OUT" -eq 1 ] || echo "building asset-tap (debug)" >&2
cargo build --quiet --manifest-path "$REPO_ROOT/Cargo.toml" --bin asset-tap >&2
"$REPO_ROOT/target/debug/asset-tap" --dump-provider-config "$PROVIDER_ID" \
    > "$CONFIG_JSON" 2> /dev/null || {
    echo "error: --dump-provider-config $PROVIDER_ID failed" >&2
    exit 2
}

# The model ids come from the dumped config, so adding a model to fal-ai.yaml is
# enough to bring it into the audit.
# bash 3.2 (macOS) has no mapfile.
MODEL_IDS=""
while IFS= read -r line; do
    MODEL_IDS="$MODEL_IDS$line
"
done < <(python3 "$REPO_ROOT/scripts/audit-fal-schemas.py" --list-ids "$CONFIG_JSON")
[ -n "$MODEL_IDS" ] || { echo "no fal models found in $YAML" >&2; exit 2; }

while IFS= read -r id; do
    [ -n "$id" ] || continue
    cache_file="$CACHE_DIR/$(printf '%s' "$id" | tr '/' '_').json"
    if [ "$REFRESH" -eq 1 ] || [ ! -s "$cache_file" ]; then
        [ "$JSON_OUT" -eq 1 ] || echo "fetching schema: $id" >&2
        if ! curl -sL --fail-with-body --max-time 60 "${SCHEMA_URL_BASE}${id}" -o "$cache_file.tmp"; then
            echo "error: failed to fetch schema for $id" >&2
            rm -f "$cache_file.tmp"
            exit 2
        fi
        if ! python3 -c 'import json,sys; json.load(open(sys.argv[1]))' "$cache_file.tmp" 2> /dev/null; then
            echo "error: non-JSON response for $id (first 200 bytes):" >&2
            head -c 200 "$cache_file.tmp" >&2
            echo >&2
            rm -f "$cache_file.tmp"
            exit 2
        fi
        mv "$cache_file.tmp" "$cache_file"
    fi
done <<< "$MODEL_IDS"

args=("$CONFIG_JSON" "$ALLOWLIST" "$CACHE_DIR")
[ "$JSON_OUT" -eq 1 ] && args+=(--json)
exec python3 "$REPO_ROOT/scripts/audit-fal-schemas.py" "${args[@]}"
