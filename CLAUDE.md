# CLAUDE.md

Guidance for Claude Code when working with this repository.

## Project Overview

**Asset Tap generates 3D models from text prompts using a two-step AI pipeline.**

**Pipeline:** Text → Image (text-to-image AI) → 3D Model (image-to-3D AI) → FBX export (Blender)

**Architecture:** Data-driven, YAML-based provider plugin system. Providers are discovered automatically - all `providers/*.yaml` files are embedded at compile time and can be edited by users at runtime.

**Cargo Workspace:**

- `core/` - Core library (provider system, pipeline orchestration, API clients)
- `cli/` - Command-line interface binary
- `gui/` - GUI application (egui + three-d 3D viewer)
- `providers/` - YAML provider configurations (embedded at compile time)
- `templates/` - YAML prompt templates (embedded at compile time)
- `bundles/` - Demo bundle assets (image + 3D model) — NOT compiled into the binary, read from disk in dev/mock mode and downloaded on demand in release

## Essential Commands

```bash
# Build
make build              # Release build (all)
make build-debug        # Debug build
make dev                # GUI debug mode (fast iteration)

# Run
make cli ARGS='-y "a cowboy ninja with a leather duster, bandana mask, and dual katanas on the back"'
make gui                # GUI release
make dev                # GUI debug

# Mock mode (zero API cost)
make mock ARGS='-y "test"'
make mock-gui

# Quality
make test                    # ALL tests (uses cargo-nextest, auto-installed if missing)
make test-cli-comprehensive  # Comprehensive CLI test suite (mock mode)
make clippy                  # Linter
make fmt                     # Format (Rust + dprint)
make verify                  # Fix everything (fmt, clippy-fix, check, test)
make ci                      # CI checks (fmt-check, clippy, check, doc, audit, test, CLI tests, site-build)
```

**Critical:** Tests run single-threaded due to template system file conflicts. This is configured automatically in `.config/nextest.toml`.

## Architecture

### Data-Driven Provider System

**Core Principle:** Providers are automatically discovered from YAML configs - no hardcoding required.

**Components:**

- `ProviderRegistry` - Discovers and loads providers from YAML
- `DynamicProvider` - Runtime provider implementation
- `HttpProviderClient` - Generic HTTP client executing provider configs
- All `providers/*.yaml` files embedded via `include_dir!` macro

**Provider Discovery:**

1. **Compile time**: All `providers/*.yaml` files automatically embedded (via `include_dir!`)
2. **First run**: Embedded configs written to user directory (`.dev/providers/` or `~/.config/asset-tap/providers/`)
3. **Runtime**: Providers loaded from user directory where they can be edited/removed

**Adding/removing providers**: Simply add/remove YAML files in `providers/` and rebuild. No code changes needed.

**Provider YAML structure:**

```yaml
provider:
  id: "provider-id"
  name: "Display Name"
  base_url: "https://api.example.com"
  env_vars: ["API_KEY"]
  upload:              # Optional: file upload config (nested under provider)
    endpoint: "/upload"
    method: POST
    request:
      type: initiate_then_put  # or "multipart"

text_to_image:
  - id: "model-id"
    endpoint: "/generate"
    method: POST
    request:
      headers:
        Authorization: "Bearer ${API_KEY}"
      body:
        prompt: "${prompt}"
    response:
      response_type: Json  # or Binary, Base64, Polling
      field: "image_url"   # JSONPath extraction
```

**Per-model tunable parameters:**

Models can declare user-tunable parameters in YAML. These are exposed as sliders/checkboxes/dropdowns in the GUI sidebar and merged into the API request body at runtime:

```yaml
text_to_image:
  - id: "model-id"
    # ... request/response config ...
    parameters:  # Optional: user-tunable fields
      - name: "guidance_scale"   # Must match a key in request.body
        label: "Guidance Scale"  # GUI label
        description: "Higher = stricter prompt adherence"
        type: float              # float, integer, boolean, string, select
        default: 3.5
        min: 1.0
        max: 20.0
        step: 0.5
      - name: "topology"
        label: "Topology"
        type: select
        default: "triangle"
        options: ["triangle", "quad"]
```

Parameter overrides are validated against declared names before injection (undeclared keys are ignored). Values persist per provider+model in `state.json` under `model_parameters`.

**CLI access:** Use `--param KEY=VALUE` (repeatable) to override parameters from the command line:

```bash
# Single param
asset-tap -y "a robot" --image-model fal-ai/flux-2 --param guidance_scale=7.0

# Multiple params
asset-tap -y "a robot" --param guidance_scale=7.0 --param num_inference_steps=10

# 3D model params
asset-tap -y "a robot" --3d-model fal-ai/meshy/v6/image-to-3d --param topology=quad --param enable_pbr=false
```

Value types are auto-detected (`true`/`false` → bool, integers, floats, or strings) and coerced to match the declared parameter type (e.g., `--param guidance_scale=7` coerces integer to float). Parameters are auto-routed to the correct model (image vs 3D) based on which model declares them. Invalid parameter names error with a list of valid options.

**Response types:**

- `Json` - Extract URL from JSON, download file
- `Binary` - Direct binary response
- `Base64` - Decode from JSON field
- `Polling` - Async with status checks

**Upload system:**

- Auto-detects when `${image_url}` in request template
- Two patterns: `multipart` (single-step) or `initiate_then_put` (two-step)
- Configured per-provider in YAML
- **Data-URI fallback:** providers with no `upload:` block get the image inlined as `data:image/png;base64,...` automatically. 10 MB cap. Used by Meshy (no upload endpoint). See [providers/meshy.yaml](providers/meshy.yaml).

**Polling with task-id providers:**

- Some providers (e.g. Meshy) return only a task id on task creation (`{"result": "<id>"}`) rather than a full status URL.
- Set `status_url_template` on `PollingConfig` to build the poll URL from the initial response. Supports `${field}`, `${field.nested}`, `${array[0]}` substitution.
- Fal uses the simpler path — `status_field` is already a full URL. Leave `status_url_template` unset.

### Template System

**Same architecture as providers - YAML-based, automatically discovered:**

**Components:**

- `TemplateRegistry` - Discovers and loads templates
- `TemplateDefinition` - Template config with variable placeholders
- `interpolation` - Variable replacement (`${var}` syntax)
- All `templates/*.yaml` files embedded via `include_dir!` macro

**Template YAML:**

```yaml
id: "template-id"
name: "Template Name"
description: "Description"
category: "character"  # or "prop", "environment", "general"
template: "Prompt text with ${variable}"
variables:
  - name: "variable"
    description: "Variable description"
    required: true
examples:
  - "example value"
```

**Variable syntax:** `${variable}`

**Adding templates:**

1. Create `templates/template-id.yaml`
2. Rebuild - automatically embedded via `include_dir!`

No code changes needed!

**Error handling:**

- Non-fatal errors collected in `REGISTRY.load_errors`
- System continues loading valid templates
- Errors shown in GUI settings modal and CLI logs

### Pipeline Orchestration

```
PipelineConfig → ProviderRegistry → Provider → HttpProviderClient → API
                                    ↓
                          Progress updates (tokio channel)
                                    ↓
                          PipelineOutput (file paths)
```

**Stages:**

1. `ImageGeneration` - Text → Image (skip if image provided)
2. `ImageTo3D` - Image → 3D (GLB format)
3. `FBXConversion` - GLB → FBX (optional, requires Blender)

**Progress tracking:** Tokio unbounded channels. Pipeline emits `Progress` enum, GUI/CLI receive.

### Bundle Structure

**Standard output structure:**

```
output/YYYY-MM-DD_HHMMSS/
├── bundle.json      # Metadata (prompt, models, costs, stats)
├── image.png        # Generated image
├── model.glb        # 3D model
├── model.fbx        # FBX (if exported)
└── textures/        # Extracted textures
```

**CRITICAL:** Filenames are ALWAYS standard (`bundle.json`, `image.png`, `model.glb`, `model.fbx`). Don't create custom names - breaks loading logic.

**Bundle naming & export:** Bundles require a custom name before export. In the GUI, the Export button is disabled until a name is set. In the CLI, use `--name`:

```bash
asset-tap -y "a robot" --name "My Robot"                          # Name at generation
asset-tap --export-bundle output/2025-01-15_143022 --name "My Robot"  # Name + export
```

**Model info:** `bundle.json` includes `model_info` (vertex count, triangle count, file size) populated automatically at pipeline time via `extract_model_info()` — no need to wait for the GUI viewer.

**Demo bundle:** A showcase 3D model (`bundles/asset-tap/`) is included in the repo but **never compiled into the binary**. It exists in two forms:

- **Dev/mock mode:** Read from disk at runtime via `env!("CARGO_MANIFEST_DIR")` (in `SampleFiles` for mock server responses)
- **Release builds:** Downloaded on demand via a button in the welcome modal or Help menu. The archive (`demo-bundle.zip`) is attached to each GitHub Release and fetched from `releases/latest/download/`. The download is atomic (temp dir + rename) to prevent partial state.

Demo bundles include a `bundle.json` with a `demo_version` field (integer, incremented only when demo content changes) and are placed in normal timestamped directories. A small `demo-manifest.json` is fetched first to check the version without downloading the full 34 MB zip. `has_demo_version()` scans local bundles for duplicates. A confirmation dialog is shown before downloading. The release workflow stamps the generator version and generates the manifest (with SHA-256 hash) from `bundle.json`. The downloaded zip is verified against the manifest hash before extraction.

**Bundle import/export:** Bundles can be exported as `.zip` archives and imported back via File > Import Bundle or the import button in the bundle info panel. `import_bundle_zip()` extracts to a temp directory, validates contents (must have image or model), creates default metadata if missing, and atomically renames to a timestamped directory. The `extract_zip_to_dir()` helper handles both import and demo download, auto-detecting and stripping a common top-level directory prefix while preserving subdirectory structure (e.g., `textures/`).

**Bundle deletion:** Bundles can be deleted from the bundle info panel via a destructive confirmation dialog that requires explicit click (no Enter shortcut).

### Dev vs Release Modes

**Dev mode** (`cfg!(debug_assertions)`):

- Settings: `.dev/settings.json`
- Output: `.dev/output/`
- Providers: `.dev/providers/` (can override embedded)
- Templates: `.dev/templates/` (can override embedded)
- Logs: `.dev/logs/`
- Uses `.env` file for API keys

**Release mode:**

- Settings: OS config dir (`~/Library/Application Support/asset-tap/` on macOS)
- Output: User-configured
- Providers: OS config dir + embedded
- Templates: OS config dir + embedded
- API keys from settings UI

**Check mode:** `is_dev_mode()` returns `cfg!(debug_assertions)`

### GUI Architecture

**Main components:**

- `App` - Main state, holds `Runtime` for async, manages pipeline state
- `ModelViewer` - three-d 3D viewer (glow/OpenGL backend)
- Views (modules under `gui/src/views/`):
  - `sidebar` - Input panel, provider/model selection
  - `preview` - Image/model/texture preview tabs
  - `progress` - Generation progress
  - `bundle_info` - Bundle metadata display
  - `library` - Browse output directory
  - `settings` - Settings modal
  - `welcome_modal` - First-run setup
  - `about` - About modal
  - `template_editor` - Template creation/editing
  - `walkthrough` - First-run walkthrough
  - `image_approval` - Image approval dialog
  - `confirmation_dialog` - Confirmation prompts

**Important:** `Arc<Mutex<SharedModelViewer>>` shares 3D viewer between egui and three-d contexts.

**Modal backdrops:** All modals use the shared `views::modal_backdrop()` helper with `BackdropClick` enum (`Close`, `CloseIf(bool)`, `Block`). Never hand-roll backdrop Area code — use the helper.

**Desktop integration:** `APP_ID` (`com.nightandwknd.asset-tap`) is set via `with_app_id()` on the viewport builder so the window manager matches the running window to the `.desktop` file from the installer. Must match `identifier` in `gui/Cargo.toml`.

**Async:** GUI spawns tokio tasks via `Runtime`. Progress flows through channels to main thread.

## Development Practices

### Adding a Provider

1. Create `providers/your-provider.yaml`
2. Rebuild - automatically embedded via `include_dir!`

No code changes needed! The `include_dir!` macro discovers all `.yaml` files at compile time.

### Testing Provider Changes

**Mock mode** validates config loading and pipeline plumbing (no API costs). It is an opt-in Cargo feature (`mock`) — **not included in release builds**. Use the Makefile targets (which enable it automatically):

```bash
make mock ARGS='-y "test"'      # CLI mock mode
make mock-gui                    # GUI mock mode
# Or build with the feature explicitly:
cargo run --features mock --bin asset-tap -- --mock -y "test"
```

Mock mode redirects all requests to a local `wiremock` server returning generic synthetic data. It verifies that YAML parses, models register, and the pipeline runs — but does **not** validate provider-specific response parsing. To test response field extraction (`response.field`), use the real API.

### Code Style

**EditorConfig enforced via dprint:**

- Rust: 4 spaces (rustfmt)
- TOML/JSON/YAML/MD: 2 spaces (dprint)
- LF line endings, UTF-8

**Formatting:** `make fmt` before committing. CI checks with `make ci`.

### Testing Best Practices

**Test execution:**

```bash
make test  # Uses cargo-nextest (single-threaded via .config/nextest.toml)
```

**Template tests:** Require single-threaded execution due to shared `.dev/templates/` file access. Configured automatically in `.config/nextest.toml`.

**Mock tests:** `make test` uses `--all-features` to include mock tests. For running mock tests individually: `make test-mock`.

**Test organization:**

- `core/src/**/*.rs` - Unit tests (inline)
- `core/tests/*.rs` - Integration tests
  - `mock_server_tests.rs` - Mock infrastructure
  - `file_io_tests.rs` - File operations
  - `pipeline_execution_tests.rs` - End-to-end
  - `integration_tests.rs` - Cross-module
  - `discovery_tests.rs` - Model discovery

**Current coverage:** ~70% overall

- Templates: ~90%
- Settings: ~85%
- Bundles: ~80%
- Mock mode: ~85%
- File I/O: ~75%
- Pipeline: ~70%
- Conversion: ~5%

## Common Gotchas

1. **Provider not found:**
   - Check that the provider YAML exists in `providers/` directory
   - Check env vars in `env_vars` field are set (providers won't be "available" without their API keys)
   - Run `cargo run --bin asset-tap -- --list-providers` to see all loaded providers

2. **Dev vs Release paths:**
   - NEVER hardcode paths
   - Use `is_dev_mode()` and appropriate path getters
   - Settings in `.dev/` vs OS config dir

3. **Async in GUI:**
   - Don't block GUI thread
   - Use `Runtime::spawn()` for long operations
   - Progress via channels, not polling

4. **Test failures:**
   - Use `make test` (nextest with single-threaded config via `.config/nextest.toml`)
   - Template tests write to shared directory
   - Clear `.dev/templates/` if tests fail unexpectedly

5. **Formatting violations:**
   - Run `make fmt` to auto-fix
   - CI enforces with `make ci`
   - dprint handles non-Rust files

6. **Embedded configs and content-compare sync:**
   - Provider/template changes require rebuild (automatically embedded via `include_dir!`)
   - User configs in config directory can be edited without rebuild
   - Remove unused provider YAML files from `providers/` to exclude them from embedding
   - **No manual version bumping.** On startup, each embedded `providers/*.yaml` and `templates/*.yaml` is compared **byte-for-byte** against its on-disk counterpart. If they differ, the on-disk copy is backed up as `.yaml.bak` and overwritten with the embedded content. The content itself is the version.
   - User-created custom files (different filenames than any embedded config) are never touched by this sync — only files whose filename matches an embedded one are compared.
   - Hand-editing an embedded config on disk is NOT a supported workflow: your edit will be backed up and reverted on the next app launch. Create a separate YAML file with a different filename instead.

7. **Packaging failures:**
   - `cargo-packager` does NOT automatically build binaries
   - Always use platform-specific targets like `make package-macos` (not `cargo packager` directly)
   - Makefile explicitly builds before packaging
   - See "Packaging & Distribution" section below for details

8. **FBX export and Blender:**
   - GUI silently skips the FBX pipeline stage when Blender is not detected (and no custom path set)
   - The user sees a "Blender not found" warning in the sidebar but the pipeline won't attempt and fail
   - CLI still attempts FBX and reports the failure in its output (acceptable for CLI UX)
   - `blender_available` is checked once at GUI startup via `find_blender()`

9. **Opening files/URLs from GUI:**
   - Always use `crate::app::open_with_system()` — never raw `open::that()`
   - Pass `Some(&mut app.toasts)` when `app` is accessible for user-visible error feedback
   - Pass `None` when inside structs without toast access (errors still log via tracing)

10. **egui/three-d version compatibility:**

- We use egui/eframe **0.33** (NOT the latest — 0.34 is available) with glow **0.16**, three-d pinned to a git rev (`da7cef6c5e17`), and three-d-asset pinned to a git rev (`edbc5d8f`)
- three-d's last crates.io release (0.18.2) only supports egui 0.29/glow 0.14 — the git master has egui 0.32/glow 0.16 support but no release has been published ([issue #568](https://github.com/asny/three-d/issues/568))
- Both git pins are in root `Cargo.toml`: three-d via `[workspace.dependencies]`, three-d-asset via `[patch.crates-io]`
- **Why not egui 0.34:** egui 0.34 requires glow **0.17**, but three-d master is on glow 0.16. No one has started porting three-d to glow 0.17. Also, `egui-phosphor` has no 0.34-compatible release yet.
- **Next upgrade path:** (1) Check if three-d has a new release or commit supporting glow 0.17. (2) Check if egui-phosphor has a 0.34-compatible release. (3) Then bump egui 0.33 → 0.34, which also involves migrating `App::update` → `App::logic`+`App::ui` and `SidePanel`/`TopBottomPanel` → unified `Panel`.
- See https://github.com/emilk/egui/discussions/113 for integration approaches

## Packaging & Distribution

**Critical:** `cargo-packager` does not build your application by default. You must build first.

**Correct workflow:**

```bash
# Use Makefile targets (recommended - builds automatically)
make package-macos           # macOS (native arch only, fast)
make package-macos-universal # macOS universal (arm64 + x86_64, release quality)
make package-windows         # Windows only
make package-linux           # Linux only

# Manual workflow (if customizing)
make build             # Build release binaries first
cd gui
cargo packager --release
```

**macOS universal binaries:** Release builds both `aarch64-apple-darwin` and `x86_64-apple-darwin`, combines with `lipo`, then packages. One DMG works on all Macs. CI builds native arch only (faster).

**macOS CLI bundling:** The CLI binary is injected into `Asset Tap.app/Contents/MacOS/` after cargo-packager creates the `.app`, then the DMG is created with `hdiutil`. Users symlink to `/usr/local/bin/` for terminal access. A standalone CLI tarball is also published for users who don't want the GUI.

**Why we use explicit build steps:**

1. **cargo-packager behavior**: By default, it doesn't build your app (see [cargo-packager docs](https://docs.crabnebula.dev/packager/))
2. **Alternative exists**: Could use `beforePackagingCommand` in `gui/Cargo.toml`, but we prefer explicit Makefile dependencies
3. **Consistency**: Matches GitHub Actions workflow pattern
4. **Clarity**: Developers can see exactly what's happening
5. **Debugging**: Easier to debug build vs packaging issues separately

**GitHub Actions workflows:**

Both CI and Release use the same macOS universal build strategy (matrix build per arch + lipo + package) to ensure parity. Linux and Windows use the shared composite action (`.github/actions/build-and-package/`).

- **CI** (`.github/workflows/ci.yaml`, PRs only): Layer 0 runs fmt, clippy, check, test, docs, audit, udeps, version-preview in parallel. Layer 1 builds macOS (arm64 + x86_64 matrix → lipo → DMG), Linux, and Windows after check passes — installer artifacts uploaded with `-pr-{N}` suffix (e.g., `asset-tap-macos-pr-7`), plus Linux binaries for CLI tests. Layer 2 runs CLI tests using the Linux binary artifact.
- **Release** (`.github/workflows/release.yaml`, push to main): CalVer versioning → parallel builds from HEAD (macOS arm64 + x86_64 as matrix jobs, Linux .deb/AppImage, Windows NSIS) → macOS packaging job combines binaries with `lipo` + creates DMG → release commit (stamps `Cargo.toml` version + generates `CHANGELOG.md`) + tag + push → GitHub Release. The release commit and tag are only created after all builds succeed.

**Dependabot** (`.github/dependabot.yml`): Cargo updates weekly (Sunday noon CST), GitHub Actions weekly. Uses `lockfile-only` versioning to avoid `Cargo.toml` churn. The entire 3D rendering stack (three-d, three-d-asset, egui, eframe, egui_extras, egui-phosphor, glow) is ignored — these are version-locked for compatibility and must be upgraded together manually (see §10 above). All minor+patch updates are grouped into a single PR; major bumps surface as individual PRs.

**Changelog:** Generated by [git-cliff](https://git-cliff.org/) from Conventional Commits. Config in `cliff.toml`. Release notes are grouped by type (Features, Bug Fixes, etc.) with merge commits and noise filtered out.

## File Locations Reference

**Dev mode:**

```
.dev/
├── settings.json
├── output/
├── providers/    # Override embedded
├── templates/    # Override embedded
└── logs/
```

**Release mode (macOS):**

```
~/Library/Application Support/asset-tap/
├── settings.json
├── providers/    # Override embedded
└── templates/    # Override embedded
```

**Output:** User-configured in settings (default: `./output` in dev, `~/Documents/Asset Tap/` in release)

## Documentation Structure

- `README.md` - Consumer-focused (installation, usage)
- `CHANGELOG.md` - Rolling changelog (auto-generated by git-cliff on release)
- `CLAUDE.md` - This file (AI development guide)
- `docs/DEVELOPMENT.md` - Developer setup, workflow
- `docs/PACKAGING.md` - Building installer packages
- `docs/architecture/PROVIDERS.md` - Provider system deep-dive
- `docs/architecture/MOCK_MODE.md` - Mock mode architecture and upload fix
- `docs/guides/BUNDLE_STRUCTURE.md` - Output format reference
- `docs/guides/PROVIDER_SCHEMA.md` - Complete YAML schema

## Key Principles

1. **Data-driven architecture:** Providers and templates are YAML configs, not code
2. **Zero-cost testing:** Mock mode for development without API costs
3. **Embedded defaults:** Configs compiled into binary, user overrides at runtime
4. **Clean separation:** Core library (reusable) vs binaries (CLI/GUI)
5. **Progressive enhancement:** GLB works without Blender, FBX optional
6. **User-friendly errors:** Template/provider errors are non-fatal, collected and displayed
7. **No conversation artifacts:** NEVER create summary/report/review markdown files - just tell the user what you did

## When Making Changes

**Before committing:**

1. Run `make verify` (formats, lints, tests)
2. Update relevant docs if adding features
3. Add tests for new functionality
4. Check `make ci` passes (CI simulation)
5. NEVER create temporary markdown files to "summarize" work - just report to the user directly

**Provider/template changes:**

1. Edit YAML in `providers/` or `templates/`
2. Rebuild to embed configs (auto-discovered by `include_dir!`)
3. Test in mock mode via `make mock` (validates config loading), then real API (validates response parsing)

**GUI changes:**

1. Test in dev mode (`make dev`) for fast iteration
2. Verify in release mode (`make gui`)
3. Check 3D viewer integration if relevant

**Architecture changes:**

1. Document in appropriate `docs/` file
2. Update CLAUDE.md if affects development workflow
3. Consider backwards compatibility
