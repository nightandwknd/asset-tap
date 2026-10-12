#!/usr/bin/env bash
# Same git-cliff invocation as .github/workflows/release.yaml. A full regen
# would rewrite history from squash commits; prepend must keep the file
# header and the latest tagged version.
set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v git-cliff >/dev/null; then
  echo "git-cliff not found — install: cargo install git-cliff"
  exit 1
fi

latest=$(git describe --tags --abbrev=0)
if ! grep -F "## ${latest} " CHANGELOG.md >/dev/null; then
  echo "ERROR: CHANGELOG.md has no heading for ${latest}"
  exit 1
fi

tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT
cp CHANGELOG.md "$tmp"

git-cliff --unreleased --tag "v0.0.0-prepend-test" --prepend "$tmp"

header_count=$(grep -c '^# Changelog' "$tmp" || true)
if [[ "$header_count" -ne 1 ]]; then
  echo "ERROR: prepend left ${header_count} '# Changelog' headings (want 1)"
  exit 1
fi

if ! grep -F "## ${latest} " "$tmp" >/dev/null; then
  echo "ERROR: prepend dropped the ${latest} heading"
  exit 1
fi

first=$(grep -m1 '^## ' "$tmp" || true)
if [[ "$first" != "## v0.0.0-prepend-test"* ]]; then
  echo "ERROR: prepend did not put the new version first (got: ${first:-empty})"
  exit 1
fi

echo "git-cliff prepend kept one header and ${latest}."

# A squash body that repeats a conventional header, as GitHub's multi-commit
# squash does ("* feat: ..."), must still render its paragraphs. git-cliff
# reads such a line as a trailer and drops everything after it.
repo=$(mktemp -d)
trap 'rm -f "$tmp"; rm -rf "$repo"' EXIT
git -C "$repo" init -q
git -C "$repo" -c user.name=t -c user.email=t@t commit -q --allow-empty -F - <<'MSG'
feat: a squashed feature (#1)

* feat: a squashed feature

Body paragraph that must survive.

* fix: a squashed fix

Second paragraph that must survive.

Co-Authored-By: Someone <someone@example.com>
MSG
notes=$(cd "$repo" && git-cliff --config "$OLDPWD/cliff.toml" --unreleased)
for want in "Body paragraph that must survive." "Second paragraph that must survive."; do
  if ! grep -F "$want" <<<"$notes" >/dev/null; then
    echo "ERROR: squash body lost \"$want\""
    echo "$notes"
    exit 1
  fi
done
if grep -E '^ +(\* )?(feat|fix): |Co-Authored-By' <<<"$notes" >/dev/null; then
  echo "ERROR: squash body kept a sub-commit header or trailer"
  echo "$notes"
  exit 1
fi
echo "git-cliff kept a squash body's paragraphs."
