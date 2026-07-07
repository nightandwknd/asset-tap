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

### Provider Allowlist

Not every provider's queue/poll/result shape is emulated by the shared mock server. Providers outside the allowlist are hidden at registry-load time when `MOCK_API=1` is set, so the GUI never offers a choice that would silently break at runtime.

The allowlist lives in `core/src/providers/registry.rs` (`MOCK_SUPPORTED_PROVIDERS`). At the time of writing it contains only `fal.ai` — Meshy's polling response uses a different `result`/`status_url_template` shape that the mock doesn't speak. The GUI's startup-time provider reconciliation (`reconcile_provider_selection` in `gui/src/app.rs`) transparently swaps a persisted Meshy selection back to the default fal.ai when mock mode is on, so users with saved state don't see a broken dropdown.

To add mock coverage for a new provider, extend `core/src/api/mock/generic_handlers.rs` and `fixtures.rs` to emit the shape the provider expects, then add the provider id to `MOCK_SUPPORTED_PROVIDERS`. When all providers have mock support we can drop the allowlist entirely.

## Synthetic Responses

JSON responses (queue/status/result) are generated by `MockFixtures` in
`fixtures.rs`. Binary file responses are the **real demo bundle assets** — mock
mode serves the actual app image and 3D model, not placeholder shapes.

### Image Generation

Serves `bundles/asset-tap/image.png` (the real ~410 KB demo image) via
`SampleFiles::minimal_png()`, which reads the file from disk at runtime. Because
these assets live on disk (never compiled into the binary) and mock mode is a
dev/CI-only feature, this works only from a repo checkout.

### File Upload

The two-step upload flow:

1. `POST .../upload` (or `.../upload/initiate`) returns `{ "upload_url": ..., "file_url": ... }`
2. `PUT` to the upload URL returns 200 OK

### 3D Generation

Serves `bundles/asset-tap/model.glb` (the real ~34 MB demo GLB, generated with
TRELLIS 2) via `SampleFiles::minimal_glb()`, again read from disk at runtime.

> Despite the `minimal_*` names, these helpers return the full demo assets, not
> minimized placeholders. The names are historical.

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
