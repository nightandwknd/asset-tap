# Vendored: devlab-theme

- Upstream: https://codeberg.org/RiPetitor/devlab-theme (MIT; LICENSE
  retained in this directory)
- Vendored from: v0.5.0, commit 8350f05 (2026-08-22)
- Excluded from the vendor copy: upstream demo content/, public/,
  screenshot.png, .git

## Local divergences (grep "VENDORED DIVERGENCE")

- sass/base/variables/_palettes.scss: brand palettes (navy/cyan +
  near-white light), --color-accent-warm, warning-callout amber family
- sass/base/_reset.scss: link hover goes warm
- sass/components/content/_prose.scss: strong text carries warm
- sass/pages/home/_hero.scss: eyebrow color warm
- sass/pages/docs/_toc.scss: active bar warm
- sass/pages/docs/_sidebar.scss: active edge warm
- templates/components/icons/github.html: official Invertocat mark
- static/js/toc.js: scroll spy clamps to the last heading at page
  bottom, and an explicit hash (URL fragment or TOC click) pins its
  link until the user scrolls; short sections clustered at the page
  end could never highlight

To update the theme: re-vendor upstream at a tagged release, then
replay the divergences above (each is a small, marked block).
