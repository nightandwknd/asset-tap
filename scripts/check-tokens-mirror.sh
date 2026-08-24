#!/usr/bin/env bash
# The theme's palette mixins mirror site/static/tokens.css (the canonical
# copy). tokens.css wins at runtime; the mixins exist so a bare theme build
# still renders on-brand. Drift between them means a stale theme build, so
# compare the color declarations of both and fail on any difference.
set -euo pipefail

cd "$(dirname "$0")/.."

TOKENS=site/static/tokens.css
MIXINS=site/themes/devlab-theme/sass/base/variables/_palettes.scss

# Extract `--name: value;` pairs, tagged by the theme they belong to.
# tokens.css: :root and [data-theme="light"] are light; the rest is dark.
extract_tokens() {
  awk '
    /^:root\[data-theme="dark"\]/ { mode="dark"; next }
    /^:root\[data-theme="light"\]/ { mode="light"; next }
    /prefers-color-scheme: dark/ { mode="dark"; next }
    /^:root \{|^:root\{/ { mode="light"; next }
    /^[[:space:]]*--color-|^[[:space:]]*--shadow-color/ {
      gsub(/^[[:space:]]+|[[:space:]]+$/, "")
      print mode " " $0
    }
  ' "$TOKENS" | sort -u
}

# _palettes.scss: one mixin per theme.
extract_mixins() {
  awk '
    /@mixin theme-palette-light/ { mode="light"; next }
    /@mixin theme-palette-dark/ { mode="dark"; next }
    /^[[:space:]]*--color-|^[[:space:]]*--shadow-color/ {
      gsub(/^[[:space:]]+|[[:space:]]+$/, "")
      print mode " " $0
    }
  ' "$MIXINS" | sort -u
}

if ! diff -u <(extract_tokens) <(extract_mixins) > /tmp/tokens-mirror.diff; then
  echo "ERROR: theme palette mixins have drifted from $TOKENS"
  echo "  (- = only in tokens.css, + = only in the mixins)"
  echo "  Canonical copy is tokens.css: edit it first, then mirror."
  echo
  cat /tmp/tokens-mirror.diff
  exit 1
fi

echo "tokens.css and the theme palette mixins agree."
