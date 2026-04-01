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
  endpoint: "https://rest.alpha.fal.ai/storage/upload/initiate"
```

The `set_base_url()` method extracts the path from absolute upload URLs and converts them to relative paths, ensuring they route through the mock server:

```
https://rest.alpha.fal.ai/storage/upload/initiate → /storage/upload/initiate
```

### Mock Server Handlers

The mock server (`core/src/api/mock/`) provides generic handlers for:

| Endpoint Pattern                                | Response                   | Purpose                       |
| ----------------------------------------------- | -------------------------- | ----------------------------- |
| `POST /*/requests/*` or similar model endpoints | Queue status JSON          | Simulate async job submission |
| `GET /*/requests/*/status`                      | Completed status           | Simulate polling              |
| `GET /*/requests/*`                             | Result JSON with file URLs | Simulate result fetch         |
| `POST /*/upload/initiate`                       | Upload URL + file URL      | Simulate two-step upload      |
| `PUT /*`                                        | 200 OK                     | Simulate file upload PUT      |
| `GET /mock-files/*`                             | Synthetic binary data      | Serve generated files         |

Handlers use regex path matching, making them provider-agnostic. Adding a new provider with different URL patterns may require adding new matchers.

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

## Synthetic Responses

### Image Generation

Returns a small synthetic PNG (a colored rectangle) served from the mock server.

### File Upload

The two-step upload flow:

1. `POST /upload/initiate` returns `{ "upload_url": "http://mock/put-here", "file_url": "http://mock/mock-files/uploaded.png" }`
2. `PUT /put-here` returns 200 OK

### 3D Generation

Returns a minimal valid GLB file (a simple triangle mesh).

### Polling

Immediate completion — the status endpoint returns `COMPLETED` on the first poll, with an optional `--mock-delay` flag to simulate real-world latency.

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

- `core/src/api/mock/mod.rs` — Mock server setup and lifecycle
- `core/src/api/mock/generic_handlers.rs` — Wiremock request matchers and response templates
- `core/src/api/mock/fixtures.rs` — Synthetic response data (PNG, GLB, JSON)
- `core/src/providers/registry.rs` — `apply_mock_mode()` — provider redirection logic
- `core/src/providers/dynamic_provider.rs` — `set_base_url()` — URL rewriting
