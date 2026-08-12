# Mock Mode Architecture

Mock mode enables full pipeline execution without API costs, providing instant feedback for development, testing, and CI/CD.

> **Note:** Mock mode is an opt-in Cargo feature (`--features mock`). It is **not compiled into release builds**. To use mock mode, build from source with the feature enabled or use the Makefile targets (which enable it automatically).

## Overview

When `MOCK_API=1` is set (or `--mock` flag is used in a mock-enabled build), the application starts a local [wiremock](https://crates.io/crates/wiremock) server and redirects all provider API traffic to it. The mock server returns synthetic responses that exercise the full pipeline: image generation, file upload, and 3D model generation.

## How It Works

### Activation

Mock mode requires the `mock` Cargo feature to be compiled in. It is triggered by:

- **CLI**: `--mock` flag or `MOCK_API=1` environment variable (requires `--features mock` build)
- **GUI**: `MOCK_API=1` environment variable (requires `--features mock` build)
- **Makefile**: `make mock`, `make mock-gui` (automatically enables the feature)

### Provider Redirection

When mock mode activates:

1. A `wiremock::MockServer` starts on a random local port
2. Each provider's `base_url` is overridden to the mock server URL
3. Absolute upload endpoint URLs are converted to relative paths so they also route through the mock server
4. Model discovery is disabled (mock API responses don't have complete model configs)

```
Normal:    Provider → https://queue.fal.run/model-id
Mock:      Provider → http://127.0.0.1:{port}/model-id
```

### URL Rewriting for Uploads

Provider configs may use absolute URLs for upload endpoints:

```yaml
upload:
  endpoint: 'https://rest.alpha.fal.ai/storage/upload/initiate'
```

The `set_base_url()` method extracts the path from absolute upload URLs and converts them to relative paths, ensuring they route through the mock server:

```
https://rest.alpha.fal.ai/storage/upload/initiate → /storage/upload/initiate
```

### Mock Server Handlers

The mock server (`core/src/api/mock/`) provides generic handlers for:

| Endpoint Pattern (regex)      | Response                    | Purpose                        |
| ----------------------------- | --------------------------- | ------------------------------ |
| `GET .*/v1/models$`           | Model list JSON             | Simulate model discovery       |
| `POST` (any path)             | Queue submission JSON       | Simulate async job submission  |
| `GET .*/requests/.*/status$`  | Status JSON (queued → done) | Simulate polling               |
| `GET .*/requests/[^/]+$`      | Result JSON with file URLs  | Simulate result fetch          |
| `POST .*/upload(/initiate)?$` | Upload URL + file URL       | Simulate upload (one/two-step) |
| `PUT` (any path)              | 200 OK                      | Simulate file upload PUT       |
| `GET .*\.(png\|jpg\|jpeg)$`   | Real demo-bundle PNG bytes  | Serve the generated image      |
| `GET .*\.glb$`                | Real demo-bundle GLB bytes  | Serve the generated 3D model   |

Handlers use regex path matching, making them provider-agnostic. They are
registered most-specific-first (in `generic_handlers::setup`), so the broad
`POST`/`PUT` catch-alls only match requests not already claimed by a more
specific pattern. Adding a new provider with different URL patterns may require
adding new matchers.

### Mock API Keys

Mock mode automatically sets environment variables for all registered providers:

```rust
for provider in registry.list_all() {
    for env_var in provider.metadata().required_env_vars {
        std::env::set_var(env_var, "mock-api-key");
    }
}
```

This ensures providers pass their "API key required" checks without real credentials.

### Discovery Disabled

Model discovery is disabled in mock mode because:

- Discovered models lack complete request/response templates
- The `create_basic_model_config()` fallback produces configs without auth headers
- Static models from YAML have complete, working configurations

Only static models defined in provider YAML files are available in mock mode.

### Every Provider Is Mock-Runnable

**Adding a provider YAML is all it takes — there is no mock code to write.**

`core/src/api/mock/config_driven.rs` reads the same `PollingConfig` the HTTP client reads and builds the exact response the client is about to ask for: it populates the fields `status_url_template` interpolates, sets `status_check_field` to `success_value`, and writes the artifact URL at `result_field` — inside `response_envelope_field` and behind `response_url_field` when those are declared. Path expressions (`images[0].url`, `model_urls.glb`) are written by the inverse of the client's field extraction, so any shape the client can read is a shape the mock can produce.

These handlers mount at wiremock priority 1, ahead of the fal-shaped catch-alls in `generic_handlers.rs`, which remain as a fallback for non-polling response types and for tests that POST to arbitrary paths.

This supersedes the `MOCK_SUPPORTED_PROVIDERS` allowlist, which registered only `fal.ai` in mock mode because the fal-shaped handlers 404'd anything with a different contract — Meshy returns a bare `{"result": "<id>"}` task id, polls a URL from `status_url_template`, and puts results at `image_urls[0]` with no envelope.

`test_every_provider_runs_in_mock_mode` in `core/tests/pipeline_execution_tests.rs` runs a full pipeline for every registered provider. A new provider whose shape the synthesizer can't build fails that test rather than silently vanishing from mock mode.

**Scope.** The mock is derived from the same YAML that drives the client, so it cannot catch a YAML that misdescribes the real API — a wrong `result_field` is wrong in both halves and still passes. Mock mode verifies config parsing, model registration, request bodies and parameter injection, the polling loop, upload/data-URI selection, artifact download, and bundle writing. Confirm response-field extraction against the real API once per provider.

## Synthetic Responses

JSON responses (queue/status/result) are generated by `MockFixtures` in
`fixtures.rs`. Binary file responses prefer the **real demo bundle assets** —
from a repo checkout, mock mode serves the actual app image and 3D model, not
placeholder shapes.

`SampleFiles` resolves the binary assets through a three-tier chain, best
available copy wins:

1. **Repo checkout** — `bundles/asset-tap/` via a path baked in at compile time
   (`env!("CARGO_MANIFEST_DIR")`), which resolves only on the machine that
   built the binary. Dev mock runs serve the real assets from here.
2. **Downloaded demo bundle** — the newest bundle whose `bundle.json` carries
   a `demo_version`, searched first in `ASSET_TAP_MOCK_DEMO_DIR` (when set),
   then in the user's configured output directory. Release users who
   downloaded the demo via the welcome modal get the same real assets in mock;
   external consumers that keep their demo bundle elsewhere point mock at it
   via the environment variable.
3. **Embedded placeholders** — a solid-color PNG and a unit-cube GLB (~1 KB
   combined) compiled into the binary, so `--mock` works anywhere instead of
   panicking.

Setting `ASSET_TAP_MOCK_EMBEDDED=1` skips both on-disk tiers; the shell suite
uses it to exercise the last-resort path from a checkout, where the disk
assets would otherwise always win.

### Image Generation

Serves `bundles/asset-tap/image.png` (the real ~410 KB demo image) via
`SampleFiles::minimal_png()`, falling back to the embedded placeholder PNG when
the demo asset is absent.

### File Upload

The two-step upload flow:

1. `POST .../upload` (or `.../upload/initiate`) returns `{ "upload_url": ..., "file_url": ... }`
2. `PUT` to the upload URL returns 200 OK

### 3D Generation

Serves `bundles/asset-tap/model.glb` (the real ~34 MB demo GLB, generated with
TRELLIS 2) via `SampleFiles::minimal_glb()`, falling back to the embedded
placeholder GLB when the demo asset is absent.

> Despite the `minimal_*` names, these helpers return the full demo assets when
> available, not minimized placeholders. The names are historical.

### Polling

The status endpoint walks through `IN_QUEUE` → `IN_PROGRESS` (with tqdm-style
log lines) → `COMPLETED` across `poll_cycles` polls (per-server counter, so
parallel tests don't interfere). `MockServerConfig::instant()` collapses this to
immediate completion; the `--mock-delay` flag adds latency to simulate
real-world timing.

## Testing with Mock Mode

```bash
# CLI
make mock ARGS='-y "test prompt"'

# GUI
make mock-gui

# Automated test suite
make test-cli-comprehensive

# Unit/integration tests (mock server used internally)
make test
```

## Architecture Diagram

```
CLI/GUI
  │
  ├─ MOCK_API=1?
  │   ├─ Yes → Start wiremock server
  │   │        Override provider base_url → localhost
  │   │        Convert absolute upload URLs → relative
  │   │        Disable discovery
  │   │        Set mock API keys
  │   │
  │   └─ No  → Use real provider URLs
  │
  └─ Pipeline runs identically in both modes
      │
      ├─ Text-to-Image → POST model endpoint → response with image URL
      ├─ Upload Image  → POST upload/initiate → PUT upload_url
      └─ Image-to-3D   → POST model endpoint → poll status → fetch result
```

## Files

- `core/src/api/mock/mod.rs` — Module root; re-exports the public mock API (`MockApiServer`, `MockServerConfig`, `SimulatedFailure`, `MockFixtures`)
- `core/src/api/mock/server.rs` — `MockApiServer` / `MockServerConfig` / `SimulatedFailure` — server startup and lifecycle
- `core/src/api/mock/generic_handlers.rs` — Wiremock request matchers and response templates (`setup()` wires all handlers)
- `core/src/api/mock/fixtures.rs` — `MockFixtures` (JSON response data) and `SampleFiles` (reads real demo-bundle PNG/GLB from disk)
- `core/src/providers/registry.rs` — `apply_mock_mode()` — provider redirection logic
- `core/src/providers/dynamic_provider.rs` — `set_base_url()` — URL rewriting
