# Changelog

All notable changes to Asset Tap are documented here.

## v26.9.7 — 2026-09-20

### Bug Fixes

- **gui:** clips return to rest on close, and Library thumbnails stop flickering ([#93](https://github.com/nightandwknd/asset-tap/pull/93))

  Previewing a clip and closing the Animation panel left the model frozen
  in the clip's last frame, and reopening the panel showed a skeleton
  still standing in that pose. Both return to rest now, and playing a clip
  again works as before.

  Opening the Library flashed a red outline over every thumbnail while
  they loaded, in development builds. Cards and their spinners now keep a
  stable identity as thumbnails arrive and as you filter, so hover and
  selection stay on the card you are pointing at.

## v26.9.6 — 2026-09-20

### Features

- --install, Meshy auto-size and decimation, dependent settings grey out ([#92](https://github.com/nightandwknd/asset-tap/pull/92))

  `--install PATH` copies the finished model.glb (or image.png under
  --image-only) out of the bundle into your project. Same field on the
  MCP generate tool.

  Meshy image-to-3D gains Auto Size, Origin, and Adaptive Decimation.
  A setting that depends on another now greys out until it applies, and
  `--param` refuses a combination the provider would reject before
  anything is generated. `--list --json` reports the rules (interface 1.2).

  --help is grouped by job, with a note on rate limits.

## v26.9.5 — 2026-09-19

### ⚠ Breaking Changes

- provider audit, Meshy 6 Lite and 7.1, --param range checks, settled positioning

### Features

- provider audit, Meshy 6 Lite and 7.1, --param range checks, settled positioning

  Meshy retired meshy-5 and deprecated meshy-7 and ultra_mode. The
  `meshy/v5/image-to-3d` model is replaced by `meshy/v6-lite/image-to-3d`
  (same parameters and credit cost); Meshy v7 now runs meshy-7.1 and
  takes `geometry_resolution` (standard/2k/4k natively, standard/2k via
  fal) instead of `ultra_mode`. Scripts that name the old id or knob need
  updating. GPT Image 2 accepts all seven aspect ratios. The dead
  `symmetry_mode` dropdown is gone. Texture prompts allow 800 characters
  on Meshy. A task Meshy cancels now fails promptly instead of polling to
  the timeout.

  `--param` enforces declared min/max and select options: an out-of-range
  value is a usage error (exit 2) with the allowed bound in the message.
  flux-2 needs at least 4 steps; flux-2 and flux-2-pro accept `seed`.

  Every fal model was checked against fal's live OpenAPI, and the check is
  now a tool: `make audit-providers` diffs the YAML against each schema
  and exits non-zero on drift, with deliberate skips recorded in an
  allowlist. Provider polling accepts a list of failure statuses.

  Releases attach `machine-interface-fixtures.zip` and a manifest so
  downstream consumers fetch and verify the golden fixtures instead of
  copying them. Installer smoke tests run only when the installer scripts
  change and after every release against the published version.

  Positioning copy is settled on one set of lines across the README, site,
  app, CLI, packages, and MCP server, single-sourced in core with a test
  that fails if any surface drifts.

## v26.9.4 — 2026-09-19

### Bug Fixes

- **site:** stop Pages marking publish as a failed deploy ([#88](https://github.com/nightandwknd/asset-tap/pull/88))

  On merge, skip preview-removal. Publish keeps open previews in the same
  commit so GitHub Pages only builds once.

- bake keeps clips a pack cannot supply, malformed glTF errors instead of panicking, and drops are never swallowed ([#89](https://github.com/nightandwknd/asset-tap/pull/89))

  Bake wrote only the clips an installed pack could supply but counted
  every ticked one, so with a pack missing it cleared the model. Re-Bind
  dropped baked clips; `bind --fit-only` re-fit a rigged mesh. Malformed
  glTF (bad indices, NaN vertices, short animation accessors) panicked
  instead of erroring. Windows could lose both copies of model.glb on a
  failed rename. Pack install is atomic and refuses ids that escape the
  packs root.

  Workbench completions carry model and request identity, so a stale
  result no longer lands on whichever bundle is current. GLB reads leave
  the UI thread. Preview and Bake retarget against the same rest;
  CUBICSPLINE clips play as written.

  Drop zones claim nothing while a dialog is open and only the files they
  use; leftovers are toasted, never swallowed.

## v26.9.3 — 2026-09-18

### Features

- **gui:** import loose GLB/image as bundles with zone drops ([#86](https://github.com/nightandwknd/asset-tap/pull/86))

  A dropped .glb or still becomes a library bundle with standard names.
  Drops land on panes: generation input, Bundle Info, empty Image/3D
  tabs, Animation packs. File → Import always makes a new bundle.

  macOS and Windows follow the cursor during a file drag. Linux cannot,
  so a still becomes the generation input, a pack installs, and
  everything else imports.

## v26.9.2 — 2026-09-16

### Bug Fixes

- **gui:** trim animation pack chrome and copy ([#84](https://github.com/nightandwknd/asset-tap/pull/84))

  Empty Animate no longer repeats Add pack. Welcome drops pack download.
  Tooltips and the confirm share one set of strings.

  Help opens the same confirm as Add pack. Hover punctuation matches.
  Site guides no longer say free packs or a hardcoded size.

### Chores

- **deps:** bump rayon in the rust-dependencies group ([#83](https://github.com/nightandwknd/asset-tap/pull/83))

## v26.9.1 — 2026-09-16

### ⚠ Breaking Changes

- rig and animate humanoid meshes in-app; drop FBX and Blender ([#82](https://github.com/nightandwknd/asset-tap/pull/82))

### Features

- rig and animate humanoid meshes in-app; drop FBX and Blender ([#82](https://github.com/nightandwknd/asset-tap/pull/82))

  Pose a shipped skeleton on a character, bind it, preview clips, and bake
  several animations into `model.glb`. Rigging needs nothing downloaded.
  Clip packs add animations, not bones.

  FBX export and Blender are gone. `--fbx`, `--no-fbx`, `--convert-fbx`,
  and `--convert-only` are usage errors. A `model.fbx` already on disk is
  left alone. Textures come out of the GLB.

  Bake writes exactly the clips you ticked. Unticking removes. Clearing
  every animation is a separate action. Adding clips to a rigged mesh
  keeps the skeleton and weights unless you pass `--refit`.

  `clip download` installs the Standard libraries from the latest release
  (hash-verified, not in the binary). Packs you already installed, including
  Source, are left alone. `clip install --from` takes a zip, folder, or
  glTF. `--clip` repeats on `bind` and `--rig`. MCP `generate` takes
  `clips: [...]`.

### Bug Fixes

- **site:** make the social preview image fetchable by X ([#78](https://github.com/nightandwknd/asset-tap/pull/78))

  The banner was a 16-bit PNG at a URL X already had a failed fetch for.
  Convert to 8-bit, serve it at a new path, and advertise width/height/type.

  Meta tags point at og-card.png; the old path was a duplicate 8-bit copy.

### Documentation

- **site:** put installation before first asset in the docs journey ([#77](https://github.com/nightandwknd/asset-tap/pull/77))

  Prev/next walks the whole sidebar as one sequence, so First Asset as
  weight 1 made generate-something the first page of the docs.

### Chores

- **changelog:** give release notes the detail they were dropping ([#80](https://github.com/nightandwknd/asset-tap/pull/80))

  The repo squash-merges, so one PR becomes one commit and one changelog
  line. That line was all anyone got: the branch's own commit messages,
  which is where the reasoning lives, were being thrown away.

  The squash body is now rendered as indented detail under its entry, with
  the trailers and separators GitHub wraps around it stripped, and a
  paragraph that merely restates the subject dropped rather than echoed.
  Breaking changes get their own section at the top instead of sitting
  unremarked among features. Groups are ordered deliberately rather than
  alphabetically, using a sort prefix that is stripped before rendering.
  Dependabot entries stay one line each, since their bodies are a list of
  version bumps nobody reads twice.

  CHANGELOG.md is regenerated so the history reads in the new format
  rather than only future releases.

  No release is cut by this: `cliff.toml`, `*.md` and `docs/**` are all in
  the release workflow's `paths-ignore`.

- **deps:** bump the rust-dependencies group with 3 updates ([#79](https://github.com/nightandwknd/asset-tap/pull/79))

- **deps:** bump the rust-dependencies group with 3 updates ([#81](https://github.com/nightandwknd/asset-tap/pull/81))

## v26.8.19 — 2026-08-31

### Features

- **site:** shared type stacks in tokens.css ([#74](https://github.com/nightandwknd/asset-tap/pull/74))

  The sans/mono stacks are brand vocabulary like the palette: both
  surfaces must set type identically. Values match the theme's own
  defaults (system stacks, no webfonts), so nothing changes on this site —
  it just makes them available to consumers of the token sheet.

- **core:** emit bundle.json v2 with artifacts and pipeline steps ([#76](https://github.com/nightandwknd/asset-tap/pull/76))

  v1 baked the text→image→3D workflow into the schema. New writes describe
  an inventory and a linear list of steps so later categories (and
  model-only / text-to-3D) don't need a new shape. v1 files stay readable
  and are not rewritten on load; config and model_info are still written
  so existing readers keep working. Generic writes omit category. CLI
  deep validation expects version 2.

  New writes omit config and model_info. Readers project prompt, models,
  params, and mesh stats from artifacts and pipeline; v1 files still load
  and are not rewritten.

### Chores

- **deps:** bump the rust-dependencies group with 2 updates ([#75](https://github.com/nightandwknd/asset-tap/pull/75))

## v26.8.18 — 2026-08-24

### Features

- **site:** shared design tokens as the canonical brand palette ([#73](https://github.com/nightandwknd/asset-tap/pull/73))

  site/static/tokens.css becomes the single source of truth for the brand's
  color vocabulary across every surface. Custom properties only: no
  selectors, no components, no layout. Vocabulary is devlab's, so the docs
  site needs no translation layer and the hub can adopt one shared
  vocabulary instead of maintaining a second.

  Linked after main.css in the site's extra_styles block, so it overrides
  the theme's stock palette at runtime. The theme's SCSS palette mixins
  survive as a mirror (a bare theme build still renders on-brand);
  scripts/check-tokens-mirror.sh compares the two and fails on drift, wired
  into make site-check / make tokens-check and both CI paths. It earned its
  keep immediately, catching --color-accent-visited present in one and not
  the other.

  Fixes a latent theming bug while here: the theme's prefers-color-scheme
  block was unguarded, so an explicit light choice on a dark-OS machine
  survived only by source order. tokens.css guards it with
  :not([data-theme="light"]).

  Verified in the browser: all three theme states (system, forced dark,
  forced light) resolve from tokens.css and the page follows.

  Step 1 of the cross-stack theme port; steps 2-4 are recorded in the hub's
  STYLE_GUIDE.md.

## v26.8.17 — 2026-08-23

### CI/CD

- gate audit on lockfile changes + daily scheduled audit [skip ci] ([#68](https://github.com/nightandwknd/asset-tap/pull/68))

- **site:** publish preserves PR previews; serialize gh-pages writers ([#71](https://github.com/nightandwknd/asset-tap/pull/71))

  The zola-deploy-action force-pushes gh-pages on every publish, which
  wiped live PR previews and made the preview-removal job push against a
  rewritten ref: the deployments page collected a failed remove-preview
  build for every merged site PR. Publish now builds with the same pinned
  Zola and deploys via JamesIves/github-pages-deploy-action with
  clean-exclude: pr-preview and no force, and a workflow concurrency
  group keeps gh-pages to one writer at a time.

### Chores

- **deps:** bump the rust-dependencies group across 1 directory with 2 updates ([#70](https://github.com/nightandwknd/asset-tap/pull/70))

### Other

- adopt devlab-theme with Asset Tap branding ([#69](https://github.com/nightandwknd/asset-tap/pull/69))

  Replace the zap theme with devlab-theme (v0.5.0, MIT, vendored at
  commit 8350f05) carrying our brand on top: navy/cyan palettes with a
  near-white light mode, a warm amber accent threaded through eyebrows,
  active markers, link hover, strong text, and warning callouts, the
  official GitHub mark, our favicons, and a footer that links Night and
  Wknd. Local theme edits are marked VENDORED DIVERGENCE in place and
  indexed in VENDORED.md with the re-vendor procedure.

## v26.8.16 — 2026-08-21

### Documentation

- **site:** MCP server page + /install.sh alias ([#67](https://github.com/nightandwknd/asset-tap/pull/67))

## v26.8.15 — 2026-08-21

### Features

- **install:** Windows one-liner — irm assettap.dev/install.ps1 | iex ([#65](https://github.com/nightandwknd/asset-tap/pull/65))

  Completes the installer story for the third platform. The script
  mirrors the bash installer's contract: downloads the CLI zip from
  GitHub Releases via redirect URLs (no API), verifies against the
  release's SHA256SUMS, installs asset-tap.exe + atap.cmd to
  %LOCALAPPDATA%\AssetTap\bin (ASSET_TAP_INSTALL_DIR overrides), pins
  via $env:ASSET_TAP_VERSION, and registers the dir on the user PATH —
  which is the Windows convention, unlike the unix script's printed
  hint. Compatible with Windows PowerShell 5.1 (preinstalled — TLS 1.2
  set explicitly, no pwsh-only syntax) and PowerShell 7.

- **gui:** window-level bundle drag & drop + bundle.json import ([#66](https://github.com/nightandwknd/asset-tap/pull/66))

  Importing a CLI-generated bundle folder into the GUI had no working
  path: the folder picker's double-click navigates (macOS behavior), and
  the input-image dropzone silently swallowed EVERY dropped file as an
  input image — so dropping a bundle folder did nothing visible.

  - Window-level drop routing: dropping a bundle folder, .zip, or a
    bundle's bundle.json anywhere on the window imports it, with a
    full-window 'Drop to import bundle' overlay while hovering. Runs
    before the panels each frame.
  - The input-image dropzone now claims only image files, and its hover
    glow lights only for image drags — the two drop targets can't steal
    or mis-signal each other's drops. Window-wide image drops stay
    (generous-target UX; scoping to the zone rect would need pointer
    tracking during native drags, which winit doesn't do reliably).
  - File → Import Bundle… also accepts a bundle's bundle.json
    (double-clicking a FOLDER in a picker can never mean select;
    double-clicking its bundle.json is unambiguous). A non-bundle .json
    pick gets a clear error instead of the zip importer's misleading
    'invalid zip archive'. Folder picker item keeps a single-click+Open
    hover note.
  - Cleanup from review: 'bundle.json' literals replaced with core's
    bundle_files::METADATA; the two divergent image-extension lists
    (dropzone 8 vs Browse picker 4) unified into one App::IMAGE_EXTS +
    is_image_file used by both.
  - Tests: predicate units for bundle-drop routing and image matching.
    Docs: 'Importing Bundles' section in using-asset-tap (drag & drop,
    both menu paths, the macOS picker note).

## v26.8.14 — 2026-08-21

### Features

- **install:** one-line CLI installer at assettap.dev/install ([#62](https://github.com/nightandwknd/asset-tap/pull/62))

  curl -fsSL https://assettap.dev/install | bash

  The CLI-first audience got the worst install path: three commands plus
  a sudo, behind an eleven-asset releases page — while every comparable
  tool (rustup, uv, bun, ollama) is a one-liner, and agents especially
  want one canonical bootstrap command.

  The script (site/static/install, served verbatim by the docs site):
  - detects platform (macOS universal, Linux x86_64; polite errors with
    the right link for Windows / Linux arm64)
  - downloads via releases/latest/download redirect URLs — no GitHub API,
    so no rate limits in CI
  - verifies against the release's SHA256SUMS before installing (most
    curl|bash installers skip this)
  - installs asset-tap + atap to ~/.local/bin — no sudo; override with
    ASSET_TAP_INSTALL_DIR; pin a release with `bash -s -- vX.Y.Z`
  - remove-then-move (macOS signature-cache kill on in-place overwrite),
    whole script wrapped in main() called last (partial-download guard),
    temp dir + cleanup trap, prints a PATH hint instead of editing rc files

### Bug Fixes

- **site:** pin zola everywhere — local builds auto-download the pinned version ([#63](https://github.com/nightandwknd/asset-tap/pull/63))

  Local 'make site-build' used whatever zola was on PATH (brew ships
  latest); zola 0.23 swapped Tera 1 for Tera 2, whose parser rejects the
  vendored zap theme's macro calls — so local site builds (and with them
  full local 'make ci') failed while CI, pinned at 0.22.1, stayed green.

  One source of truth now: ZOLA_VERSION in the Makefile. Site targets
  depend on a pinned binary auto-downloaded once into site/.bin/
  (gitignored) — same auto-install philosophy as cargo-nextest — so
  local builds match the deploy exactly regardless of what brew has.
  The two pins in site.yaml (taiki-e zola@ and the zola-deploy-action
  tag) carry keep-in-sync comments referencing the Makefile var, same
  convention as the toolchain/Dockerfile dual pin.

  Porting the theme to zola 0.23/Tera 2 ('components' replace macros
  wholesale) is real migration work with visual-regression risk — queued
  separately, not smuggled into a version pin.

## v26.8.13 — 2026-08-20

### Bug Fixes

- **bundle:** CLI-output → GUI-library path — Finder-zip tolerance, folder import, outside-library hint ([#61](https://github.com/nightandwknd/asset-tap/pull/61))

  A bundle generated with a custom -o had no clean route into the GUI
  library: the GUI only imported zips, and zipping the folder with macOS
  Archive Utility produced an archive the importer rejected ('Bundle must
  contain at least an image or model') — the parallel __MACOSX/ tree adds
  a second top-level directory, which defeated wrapper-folder flattening,
  so content stayed nested and root validation found nothing.

  Three fixes, one per gap:

  - import: skip macOS archive junk (__MACOSX/ trees, ._* AppleDouble
    files, .DS_Store) in both extraction passes, restoring wrapper
    flattening for Archive-Utility zips. Regression test's entry list is
    modeled on a real Archive Utility archive.
  - gui: File → Import Bundle Folder… imports a bundle directory
    directly — no archive step. New core import_bundle_dir() copies
    (source untouched), skips junk, enforces the extraction entry cap,
    skips symlinks, and shares validation/metadata/finalize with zip
    import via a factored finalize_imported_bundle().
  - cli: when -o lands outside the configured library, the completion
    summary now says so and points at the GUI import (or omitting -o),
    instead of letting the user discover the gap as a failed hunt.

## v26.8.12 — 2026-08-20

### ⚠ Breaking Changes

- **cli:** FBX conversion is opt-in — GLB-only by default on every surface ([#60](https://github.com/nightandwknd/asset-tap/pull/60))
  bare `asset-tap "prompt"` now produces GLB only.
  Pass --fbx (CLI) or fbx: true (MCP) for FBX.

### Bug Fixes

- **cli:** FBX conversion is opt-in — GLB-only by default on every surface ([#60](https://github.com/nightandwknd/asset-tap/pull/60))

  The CLI was the only surface that exported FBX by default (flag spelled
  --no-fbx), while the GUI checkbox defaults off (export_fbx_default:
  false) and the MCP tool already defaulted to GLB-only. Same product,
  opposite defaults — and the CLI default front-loaded a Blender
  dependency most users don't have.

  - core: PipelineConfig::new() no longer forces export_fbx: true; new
    with_fbx() builder. FBX failure already degrades gracefully (stage
    fails, GLB kept), unchanged.
  - cli: new --fbx flag opts in; --no-fbx is kept as a hidden, deprecated
    no-op (prints a stderr note) so existing scripts and agent snippets
    keep working — the comprehensive suite's 40+ --no-fbx usages pass
    unchanged as proof. --fbx conflicts with --no-fbx (usage error).
  - mcp: new fbx param (default false); older clients passing
    no_fbx: false still opt in (field deprecated but honored). --no-fbx
    is never emitted into the argv anymore.
  - gui: the enabled path now opts in explicitly with with_fbx() — it
    previously relied on the core default being true, which would have
    silently broken the checkbox.
  - docs: AGENTS.md, MCP.md, CLI_MACHINE_INTERFACE.md, site cli-usage
    all teach --fbx / fbx: true.

## v26.8.11 — 2026-08-20

### Features

- **providers:** Meshy v7 via fal, Smart Topology, remove_background ([#58](https://github.com/nightandwknd/asset-tap/pull/58))

  - fal.ai: add Meshy v7. fal serves it under the partner namespace
    (endpoint meshy/v7/image-to-3d); our model id keeps the fal-ai/
    prefix so it can't collide with the native meshy provider's id in
    cross-provider model routing. Surface at parity with the native v7
    block: fal's schema passes texture_prompt through but not
    texture_resolution / image_enhancement.
  - meshy: add Smart Topology (meshy-t2) — Meshy's recommended
    replacement for the deprecated lowpoly mode: clean topology, natively
    separated parts, settable face count with no remesh pass.
    Live-verified: meshy-t2 caps target_polycount at 15,000 (not in
    Meshy's docs); range and default set accordingly.
  - extend the cross-provider drift test to cover the v7 pair (fal
    wrapper vs native surface must stay in lockstep, minus the two
    documented native-only params).
  - audit note: native text-to-image (nano-banana ×3, gpt-image-2) and
    standard image-to-3D (v5/v6/v7) verified complete against Meshy's
    current docs — no other gaps.

### Documentation

- document the `atap` alias in install and CLI docs (README, AGENTS.md, site) ([#57](https://github.com/nightandwknd/asset-tap/pull/57))

  `atap` shipped in v26.8.5 as a symlink/`atap.cmd` in every CLI archive but
  was documented only in PACKAGING.md (maintainer-facing) and the changelog.
  Worse, the README's and the site's standalone-CLI install steps
  (`sudo mv asset-tap /usr/local/bin/`) left the alias behind in the
  extracted archive — following the docs, you never got it.

  - README: install steps move both (`sudo mv asset-tap atap …`); the DMG's
    bundled-CLI symlink gets an optional `atap` twin; .deb note (installs
    the alias system-wide); Windows zip note (`atap.cmd`); a CLI Usage
    intro line — release archives ship it, source builds don't (add your
    own alias).
  - AGENTS.md: agents can use the shorter name; file keeps `asset-tap` for
    clarity.
  - Site: installation.md mirrors the README fixes; cli-usage.md gets an
    alias callout.

  Accurate to the packaging scripts: tarballs/.deb carry the symlink, the
  Windows zip carries atap.cmd, the DMG-bundled CLI and the Windows
  setup.exe do not include it, source builds don't either.

### Chores

- **deps:** bump h2 0.4.13 -> 0.4.17 (RUSTSEC-2026-0258) ([#59](https://github.com/nightandwknd/asset-tap/pull/59))

## v26.8.10 — 2026-08-18

### Features

- **cli:** `asset-tap mcp` — Model Context Protocol server over stdio, a thin front door over the CLI's own code paths ([#56](https://github.com/nightandwknd/asset-tap/pull/56))

  For agent hosts without a shell (Claude Desktop, Cursor, IDE agents).
  Built on rmcp (the official Rust MCP SDK). Four tools, each 1:1 onto
  something the CLI already does, returning the same shapes as the --json
  wire format so the two can't drift:

  - list_catalog → machine::build_catalog (== `--list --json`)
  - auth_status → machine::AuthCatalog (== `auth list --json`; no keys)
  - inspect_bundle → bundle.json + file list
  - generate → builds an argv from the tool args and runs it through
    the same clap Cli parser, resolve_param_overrides, and run_generation
    as the binary — validation, model resolution, --param routing, and
    error classification are literally the CLI's; usage errors are
    word-for-word the CLI's. Progress → MCP notifications/progress through
    one ordered channel (all delivered before the result); cancellation
    from the request's token; nothing on stdout (it's the transport).

  The only refactor: run_generation takes a RunSink — Cli (unchanged
  behavior) or Embedded { on_progress, cancel }. non_interactive_input_check
  is shared so the MCP gives the CLI's exact "requires a prompt or --image"
  usage error. Provider keys are synced into env once at server start (as
  the CLI/GUI do); per-call env mutation isn't sound with the runtime live.

## v26.8.9 — 2026-08-16

### Features

- **cli:** agent-ergonomics pass — auth list --json, feature-gated --mock example, AGENTS.md ([#55](https://github.com/nightandwknd/asset-tap/pull/55))

  The CLI is already agent-legible by design (spec §7: --machine-help,
  examples block, exit codes, env-var auth, stdout/stderr contract,
  --version --json). This closes the remaining edges an agent hits:

  - `asset-tap auth list --json`: a single JSON document for key preflight —
    per provider `configured`, `source` (stored|env|missing), `env_var`,
    `required_env_vars`. Never carries key material. Spec §3, golden
    fixture `auth_catalog.json` (drift-tested), unit-tested for stored/env/
    missing under env_lock(). New document under interface 1.0 (no
    existing shape changed).
  - The `--mock --json` example is compiled out of `--help` when the `mock`
    feature is absent: release binaries don't have the flag, and help must
    never advertise an argument the binary rejects. Spec §7 says so.
  - AGENTS.md: one-read orientation for coding agents — using the CLI
    (always --json, prompt required, read result + exit codes, the bundle is
    the product, prefer --no-fbx, idempotency, cancellation, no-cost calls
    against a release binary) and working on the repo. README points to it.

  Review pass on the branch:
  - DRY: the stored/env/missing resolution existed twice (human `auth
    list` in main.rs and AuthCatalog::collect). Both now render from one
    collected AuthCatalog; the resolution is `machine::KeySource::resolve`.
  - Magic strings: `KeySource` enum with STORED/ENV/MISSING constants and
    `as_str()`/`is_configured()`; the wire format is unchanged. Tests use
    the constants. New unit test pins the precedence (stored beats env;
    empty stored counts as absent; env only when non-empty).
  - AGENTS.md: verified against the binary — `--name` sets bundle.json's
    `name` (needed for --export-bundle), NOT the directory; runs never
    overwrite (timestamped dirs, -1/-2 suffixes on collision). Reworded.

## v26.8.8 — 2026-08-16

### Chores

- bump Rust toolchain from 1.94.1 to 1.97.1 ([#54](https://github.com/nightandwknd/asset-tap/pull/54))

## v26.8.7 — 2026-08-13

### Bug Fixes

- **site:** pin zola-deploy-action to v0.22.1 until zap theme supports Zola 0.23

  Zola 0.23 dropped Tera template syntax, which the vendored zap theme
  uses. Align the PR preview job to the same zola version so previews
  test what publish deploys, and ignore the action in Dependabot until
  the theme is ported.

### CI/CD

- **ci:** harden release publish against stale reruns

  Fail fast when main has moved past the run's SHA (a newer push's
  release supersedes it), and push the release commit and tag atomically
  so a rejected branch push can't strand a half-pushed tag.

### Chores

- **deps:** bump the rust-dependencies group across 1 directory with 2 updates ([#53](https://github.com/nightandwknd/asset-tap/pull/53))

- **deps:** bump shalzz/zola-deploy-action ([#52](https://github.com/nightandwknd/asset-tap/pull/52))

## v26.8.6 — 2026-08-13

### CI/CD

- **ci:** lock-consistency guard + Dependabot grouping fixes

  - `make lock-check` (cargo metadata --locked), first in `make ci` and a step
    in the CI Check job. Under versioning-strategy lockfile-only, a Dependabot
    major bump rewrites Cargo.lock while the manifest still forbids the new
    version — cargo silently re-resolves and CI stays green while the
    "upgrade" does nothing. Fail loudly instead.
  - Dependabot: security updates get their own groups (they bypass
    version-update groups otherwise, arriving as individual PRs); the actions
    group now covers majors (checkout v6→v7 class bumps are routine).
  - Repo auto-merge enabled so grouped dep PRs can merge on green.

### Chores

- **deps:** update webbrowser to 1.2.4 (RUSTSEC-2026-0257)

## v26.8.5 — 2026-08-12

### Features

- **mock:** serve downloaded demo bundle assets when repo checkout is absent

  Follow-up to #48, which made released binaries fall back from the repo's
  demo assets straight to ~1 KB embedded placeholders. Release users who have
  downloaded the demo bundle already have the real assets on disk, so mock
  mode should show them the actual image and model, not a placeholder cube.

  SampleFiles now resolves through a three-tier chain, best copy wins:

  1. Repo checkout (bundles/asset-tap via the compile-time path) — dev runs
  2. Newest downloaded demo bundle in the output directory, found via the
     demo_version field in bundle.json (same discovery the demo download uses)
  3. Embedded placeholders — last resort, so --mock works anywhere

  ASSET_TAP_MOCK_EMBEDDED=1 now skips both disk tiers, keeping the CI guard
  for the last-resort path meaningful. Unit tests cover the demo tier (newest
  demo_version wins, plain bundles ignored, missing output dir is a clean
  miss). Docs updated to describe the chain.

- **mock:** ASSET_TAP_MOCK_DEMO_DIR points mock at an external demo bundle dir

  External consumers that keep their downloaded demo bundle outside the
  configured output directory can now point mock mode at it. The directory is
  scanned the same way as the output directory (newest demo_version wins) and
  takes precedence over it; embedded placeholders remain the last resort.

- **cli:** `demo download` subcommand

  The showcase demo bundle was downloadable only through the GUI's welcome
  modal, even though the capability lives in core. CLI-only installs had no
  way to fetch it — which also meant mock mode could never show them real
  assets. `asset-tap demo download [-o DIR]` exposes the existing core
  function: manifest version check, skip-if-present, SHA-256 verification,
  atomic extract. Defaults to the configured output directory.

  --json is rejected with the subcommand (exit 2), matching auth; network
  failures exit with the network code (6) per the exit-code table.

- **packaging:** ship `atap` alias alongside the CLI in all release archives

  `asset-tap` stays the canonical binary name; `atap` is a typing convenience:

  - macOS/Linux tarballs and the .deb's usr/bin: relative symlink
  - Windows zip: atap.cmd forwarding shim (zip archives can't carry symlinks)

  Signing/notarization attach to the target binary, so the symlink needs
  neither. Verified locally: symlink survives tar round-trip and invokes the
  binary; actionlint + shellcheck clean.

## v26.8.4 — 2026-08-12

### Bug Fixes

- mock mode panics in released binaries (missing demo assets) ([#48](https://github.com/nightandwknd/asset-tap/pull/48))

  SampleFiles resolved the demo bundle assets via env!("CARGO_MANIFEST_DIR"),
  a path baked in at compile time that only exists on the build machine. Since
  releases ship with the mock feature enabled (release.yaml builds with
  --features mock), any released binary panicked on --mock: exit 101, no NDJSON
  events, from every directory on every user machine. CI never caught it
  because it builds and runs in the same workspace, so the baked path always
  resolves there.

## v26.8.3 — 2026-08-11

### Features

- **providers:** add Meshy v7 image-to-3D with ultra_mode support ([#47](https://github.com/nightandwknd/asset-tap/pull/47))

  Add meshy/v7/image-to-3d to the native Meshy provider and make it the
  provider default, matching Meshy's own `latest` alias (which now
  resolves to meshy-7). Verified against Meshy's API reference and a live
  generation (30 credits textured — same as v6; task accepted, polled to
  SUCCEEDED, GLB parsed and downloaded).

  Surface deltas vs v6, each traced to Meshy's docs:

  - ultra_mode (new, v7-only): higher-fidelity geometry
  - remove_lighting omitted: documented "Only supported when ai_model is
    meshy-6"
  - symmetry_mode not advertised: deprecated API-wide ("no longer affects
    output"); kept on v5/v6 for continuity
  - texture_resolution / image_enhancement retained (meshy-6-or-later)
  - should_remesh defaults false (clean topology is native, like v6)

  fal has no v7 wrapper (endpoint 404s; catalog tops out at v6), so v7 is
  native-only.

### Chores

- **deps:** bump open in the rust-dependencies group ([#46](https://github.com/nightandwknd/asset-tap/pull/46))

## v26.8.2 — 2026-08-02

### Bug Fixes

- **gui:** report conflicting generation settings instead of running anyway ([#45](https://github.com/nightandwknd/asset-tap/pull/45))

  Checking "Image only (skip 3D)" and then choosing an input image left the
  checkbox checked but disabled, with no way to clear it. Generate stayed
  enabled, and `build_config` dropped `skip_3d` because an image was present, so
  the run produced a 3D model while the sidebar said image-only.

  - The checkbox stays interactive when it conflicts with an input image.
    Disabling it was what made the state unrecoverable.
  - Neither selection is cleared automatically. Both are the user's; silently
    undoing one hides the mistake rather than surfacing it.
  - Generate is disabled while the two conflict, with the reason on hover, and
    an inline warning appears under the checkbox where the choice was made.
  - `build_config` honors `skip_3d` as selected. It previously dropped the flag
    when an image was set, which is what let the UI and the run disagree.
  - `can_generate` no longer requires a 3D model in image-only mode, where the
    3D stage never runs.

  Also moves the toggle out of Post-Processing and into 3D Generation. It
  decides whether that stage runs, which is pipeline scope rather than something
  applied to a finished model. It sits outside that section's `add_enabled_ui`,
  or enabling it would disable the control needed to turn it back off.

  Separately, an input image that has been moved or deleted since it was picked
  is now caught before the run starts, matching the CLI. The path previously
  fell through to the pipeline's remote-URL branch and failed as a download
  error.

### Documentation

- **site:** correct inaccurate reference material and document tunable parameters

  Audited every claim on the docs site against the CLI, the provider YAML, and
  the GUI source. Corrections:

  - `--export-bundle` example failed as written; bundles need a name first.
  - Mouse zoom was documented as Ctrl/Cmd+Scroll. The wheel zooms with no
    modifier; Ctrl/Cmd is the trackpad modifier. Added middle-drag orbit and
    Shift+middle pan, which were undocumented.
  - Removed a Keyboard Shortcuts table listing a Generate-on-Enter shortcut that
    has no handler, alongside an entry that wasn't a shortcut.
  - `bundle.json` example was missing `duration_ms`, `tags`, `favorite`, `notes`
    and `generator`, plus `template` and `existing_image` under config.
  - `image_model_params` / `model_3d_params` were described as user overrides
    omitted when empty. They record the effective values — declared defaults
    with overrides applied — so a bundle stays reproducible.
  - `name` defaults to null, not the prompt.
  - Meshy tables listed two of four text-to-image models, one aspect-ratio set
    for all of them, and an incomplete image-to-3D parameter list. Added model
    ids so the tables show what to pass to `--image-model` / `--3d-model`.
  - Documented the `parameters:` block in the schema reference. The per-model
    tunable parameter system had no public documentation.
  - `--param` section now covers empty-value clearing, per-stage validation
    scope, and exit code 2.
  - Mock mode notes updated for config-driven handlers covering every provider.

## v26.8.1 — 2026-08-01

### Bug Fixes

- correct CLI validation scope, exit codes, and provider catalog ([#44](https://github.com/nightandwknd/asset-tap/pull/44))

  Fixes wrong `--param` validation, exit codes that misreported the failure
  class, JPEG bytes written as `image.png`, mock mode covering only one
  provider, and missing catalog entries.

  - `--param` validates only against the models a run uses, resolved the way
    the pipeline resolves them (respecting `-p`). `--image-only` no longer
    rejects `aspect_ratio` or lists 3D parameters from an unused provider.
  - Bad `--param` values exit 2 and unknown model ids exit 4, instead of 1.
  - `image.png` always contains PNG bytes. Meshy's text-to-image returns JPEG,
    which was written under the `.png` name and passed to the 3D stage as a
    `data:image/png` URI.
  - Mock handlers are synthesized from each provider's polling contract,
    replacing the `MOCK_SUPPORTED_PROVIDERS` allowlist. Adding a provider YAML
    is now enough to make it mock-runnable, and a test covers every registered
    provider.
  - Catalog adds `meshy/nano-banana-2`, `meshy/gpt-image-2` (1:1/3:2/2:3),
    Meshy v6 production parameters, and fal `seed`, each checked against the
    provider's API reference.
  - `allow_unset` lets a GUI dropdown clear a select to null, required for
    Meshy's mutually exclusive Multi-View and aspect ratio.

### Chores

- **deps:** bump serde_json in the rust-dependencies group ([#43](https://github.com/nightandwknd/asset-tap/pull/43))

## v26.7.3 — 2026-07-23

### Chores

- **deps:** bump the rust-dependencies group across 1 directory with 2 updates ([#40](https://github.com/nightandwknd/asset-tap/pull/40))

- **deps:** bump the rust-dependencies group with 4 updates ([#42](https://github.com/nightandwknd/asset-tap/pull/42))

- **deps:** move 3D stack to crates.io releases (egui 0.34, three-d 0.19) ([#41](https://github.com/nightandwknd/asset-tap/pull/41))

## v26.7.2 — 2026-07-14

### Features

- machine-readable CLI interface (--json) for external tools ([#39](https://github.com/nightandwknd/asset-tap/pull/39))

  Adds a versioned NDJSON wire format so external tools and integrations can
  drive asset-tap as a subprocess without screen-scraping human output. Spec:
  docs/CLI_MACHINE_INTERFACE.md (also embedded in the binary via
  --machine-help / --describe).

  Interface (wire v"1.0", Terraform-style MAJOR.MINOR semantics):
  - `--json`: NDJSON events on stdout (start / progress / log / one authoritative
    result: success|error|canceled); all diagnostics on stderr; implies --yes.
  - Differentiated exit codes (spec §2): 2 usage, 3 auth, 4 provider, 5 canceled
    (--json; human cancellation keeps the 130 shell convention), 6 network,
    7 local-env. Codes apply in human mode too (previously always 1).
  - Machine catalog: `--list --json` / `--list-providers --json` emit providers,
    models, typed tunable parameters (ranges/options/defaults), and templates.
    The human --list-providers output renders from the same traversal.
  - `--version --json` → {"version","interface"} for two-axis compat checks.
  - Agent ergonomics (§7): examples/auth/exit codes in --help; --machine-help
    embeds the full spec so tools can discover the contract offline.

  Core changes in support:
  - Typed cancellation: Error::Cancelled + ApiErrorKind::Cancelled with a single
    Error::is_cancellation() source of truth (pipeline, http_client, and
    provider-side "canceled" responses) — no more message-text matching.
  - Stage::wire_name() single-sources wire stage names.
  - config_sync reseeds via atomic tmp+rename writes (was truncating fs::write).
  - bundle_dir in the success result is contractually absolute (errors instead
    of silently emitting relative/empty paths).
  - Graceful SIGINT/SIGTERM: first signal cancels, second force-quits with
    stdout flushed; a cancel landing after the last pipeline check reports
    canceled, not success; stage context tracks Started/Completed/Failed.

## v26.7.1 — 2026-07-07

### Bug Fixes

- security hardening, robustness, and parallel test suite ([#36](https://github.com/nightandwknd/asset-tap/pull/36))

### Chores

- **deps:** bump rpassword in the rust-dependencies group ([#27](https://github.com/nightandwknd/asset-tap/pull/27))

- **deps:** bump the rust-dependencies group across 1 directory with 3 updates ([#30](https://github.com/nightandwknd/asset-tap/pull/30))

- **deps:** bump the rust-dependencies group with 2 updates ([#31](https://github.com/nightandwknd/asset-tap/pull/31))

- **deps:** bump actions/checkout from 6 to 7 ([#34](https://github.com/nightandwknd/asset-tap/pull/34))

- **deps:** bump the rust-dependencies group across 1 directory with 2 updates ([#33](https://github.com/nightandwknd/asset-tap/pull/33))

- **deps:** bump the rust-dependencies group with 2 updates ([#35](https://github.com/nightandwknd/asset-tap/pull/35))

- **deps:** bump open in the rust-dependencies group ([#37](https://github.com/nightandwknd/asset-tap/pull/37))

## v26.4.18 — 2026-04-28

### Features

- provider parameter and widget audit ([#26](https://github.com/nightandwknd/asset-tap/pull/26))

### Chores

- doc site updates

## v26.4.17 — 2026-04-21

### Chores

- lint/format hardening + preserve gui entitlements on bundle re-sign ([#25](https://github.com/nightandwknd/asset-tap/pull/25))

## v26.4.16 — 2026-04-21

### Features

- sign and notarize macos releases with developer id ([#23](https://github.com/nightandwknd/asset-tap/pull/23))

### Bug Fixes

- add workflow timeouts and fix cli notarization format

### Chores

- **deps:** bump the rust-dependencies group with 3 updates ([#24](https://github.com/nightandwknd/asset-tap/pull/24))

## v26.4.15 — 2026-04-20

### Features

- image-only mode and library generation shortcuts ([#22](https://github.com/nightandwknd/asset-tap/pull/22))

## v26.4.14 — 2026-04-19

### Features

- add cli auth subcommand and persist effective model params ([#21](https://github.com/nightandwknd/asset-tap/pull/21))

## v26.4.13 — 2026-04-15

### Bug Fixes

- align provider defaults and persist model params ([#20](https://github.com/nightandwknd/asset-tap/pull/20))

### Chores

- **deps:** bump indexmap in the rust-dependencies group ([#18](https://github.com/nightandwknd/asset-tap/pull/18))

## v26.4.12 — 2026-04-14

### Features

- add meshy provider config ([#19](https://github.com/nightandwknd/asset-tap/pull/19))

## v26.4.11 — 2026-04-12

### Features

- settings hardening, content-compare sync, mock test speedup ([#17](https://github.com/nightandwknd/asset-tap/pull/17))

### Chores

- update dependabot config

## v26.4.10 — 2026-04-04

### Chores

- **deps:** bump indexmap in the rust-minor-patch group ([#15](https://github.com/nightandwknd/asset-tap/pull/15))

## v26.4.9 — 2026-04-04

### Features

- versioned demo bundles, bundle importer, delete, integrity checks ([#16](https://github.com/nightandwknd/asset-tap/pull/16))

### Bug Fixes

- release workflow bundle.json

## v26.4.7 — 2026-04-03

### Features

- strip demo assets from binary; download on demand ([#14](https://github.com/nightandwknd/asset-tap/pull/14))

## v26.4.6 — 2026-04-02

### Chores

- **deps:** bump the rust-minor-patch group with 5 updates ([#13](https://github.com/nightandwknd/asset-tap/pull/13))

## v26.4.5 — 2026-04-02

### Chores

- **deps:** bump tracing-subscriber in the tracing group ([#12](https://github.com/nightandwknd/asset-tap/pull/12))

## v26.4.4 — 2026-04-02

### CI/CD

- ignore rust-toolchain in dependabot

### Chores

- **deps:** bump tokio from 1.49.0 to 1.50.0 in the tokio group ([#11](https://github.com/nightandwknd/asset-tap/pull/11))

## v26.4.3 — 2026-04-02

### CI/CD

- add dependabot config; simplify workflow needs chains ([#9](https://github.com/nightandwknd/asset-tap/pull/9))

## v26.4.2 — 2026-04-02

### Features

- model tunable parameters, param cli flag, bundle improvements, rust toolchain pin ([#7](https://github.com/nightandwknd/asset-tap/pull/7))

## v26.4.1 — 2026-04-01

### Features

- strip mock mode from release builds ([#8](https://github.com/nightandwknd/asset-tap/pull/8))

## v26.3.6 — 2026-03-28

### Features

- upgrade egui to 0.33 ([#6](https://github.com/nightandwknd/asset-tap/pull/6))

### Documentation

- add macos gatekeeper workaround

## v26.3.5 — 2026-03-28

### Features

- upgrade egui to 0.32 + three-d to git rev ([#5](https://github.com/nightandwknd/asset-tap/pull/5))

## v26.3.4 — 2026-03-25

### Bug Fixes

- ref image metadata, changelog fmt, approval progress, dmg packaging ([#4](https://github.com/nightandwknd/asset-tap/pull/4))

## v26.3.3 — 2026-03-25

### Features

- post-pipeline fbx conversion for gui and cli ([#3](https://github.com/nightandwknd/asset-tap/pull/3))

### Bug Fixes

- add meta tags for x link previews

## v26.3.2 — 2026-03-22

### Bug Fixes

- site links to release artifacts

- codesign, binary size, and macOS install docs ([#2](https://github.com/nightandwknd/asset-tap/pull/2))

## v26.3.1 — 2026-03-22

### Features

- build asset tap ([#1](https://github.com/nightandwknd/asset-tap/pull/1))

### Chores

- release fixes and updates
