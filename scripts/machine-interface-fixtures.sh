#!/usr/bin/env bash
# Package the machine-interface golden fixtures as a release asset so consumers
# fetch and verify them instead of copying from a sibling checkout.
#
# Usage: scripts/machine-interface-fixtures.sh OUTPUT_DIR
#
# Writes OUTPUT_DIR/machine-interface-fixtures.zip (flat, no top-level dir,
# README.md included) and OUTPUT_DIR/machine-interface-fixtures-manifest.json.
#
# The interface version is grepped from cli/src/machine.rs INTERFACE_VERSION
# rather than passed in: the constant is the single source, and a workflow
# argument would be a second place to forget to bump.
set -euo pipefail

cd "$(dirname "$0")/.."

out_dir=${1:-}
if [[ -z "$out_dir" ]]; then
  echo "usage: $0 OUTPUT_DIR"
  exit 1
fi

if ! command -v zip >/dev/null; then
  echo "ERROR: zip not found"
  exit 1
fi

fixtures_dir="cli/tests/fixtures/machine-interface"

interface=$(sed -n 's/^pub const INTERFACE_VERSION: &str = "\(.*\)";$/\1/p' cli/src/machine.rs)
if [[ -z "$interface" ]]; then
  echo "ERROR: could not read INTERFACE_VERSION from cli/src/machine.rs"
  exit 1
fi

sha256() {
  if command -v sha256sum >/dev/null; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

mkdir -p "$out_dir"
out_dir=$(cd "$out_dir" && pwd)
zip_path="$out_dir/machine-interface-fixtures.zip"
manifest_path="$out_dir/machine-interface-fixtures-manifest.json"

# Sorted, so the archive's member order does not depend on readdir order.
# A read loop rather than mapfile: macOS ships bash 3.2.
names=()
while IFS= read -r name; do
  names+=("$name")
done < <(cd "$fixtures_dir" && for f in *; do [[ -f "$f" ]] && printf '%s\n' "$f"; done | LC_ALL=C sort)

rm -f "$zip_path"
# -X drops extra file attributes (uid/gid, timestamps) for a reproducible zip.
(cd "$fixtures_dir" && zip -qX "$zip_path" "${names[@]}")

files_json=""
for name in "${names[@]}"; do
  hash=$(sha256 "$fixtures_dir/$name")
  files_json+=$(printf '{"name":"%s","sha256":"%s"}' "$name" "$hash")
done

zip_sha=$(sha256 "$zip_path")

printf '%s' "$files_json" \
  | jq -s --arg interface "$interface" --arg sha "$zip_sha" \
    '{interface: $interface, files: (map({(.name): .sha256}) | add), sha256: $sha}' \
    > "$manifest_path"

echo "Wrote $zip_path (${#names[@]} files) and $manifest_path"
