# Provider System Architecture

The asset tap features a flexible, extensible plugin system for AI model providers through YAML configuration files.

## Overview

```
Pipeline
  └─> ProviderRegistry (Discovery)
      ├─> fal.ai (YAML config)
      ├─> Meshy AI (YAML config)
      └─> Custom providers (user-defined)
```

## How It Works

Providers are defined via YAML configuration files that specify:

- Provider metadata (ID, name, API key requirements)
- Text-to-image models and endpoints
- Image-to-3D models and endpoints
- Upload configuration (if the API requires public image URLs)
- Request/response templates

The system loads these configs at runtime and executes them through a generic HTTP client. No code changes needed to add new providers.

For complete YAML schema details, see [Provider Schema Reference](../guides/PROVIDER_SCHEMA.md).

## Response Types

The system supports different API response patterns:

- **Json** - Extract a URL from JSON response and download the file
- **Binary** - Direct binary response (raw image/model data)
- **Base64** - Decode base64-encoded data from JSON
- **Polling** - Async operations with status checks (for long-running jobs)

See [Provider Schema Reference](../guides/PROVIDER_SCHEMA.md#response-types) for configuration details.

## Upload System

Some providers require public URLs for image inputs instead of accepting direct file uploads. The upload system handles this automatically.

**How it works:**

1. System detects `${image_url}` placeholder in request template
2. Uploads image using provider's upload configuration
3. Gets back a public URL
4. Substitutes URL into the request

**Upload patterns supported:**

- **Multipart** - Single-step upload (see provider configs for examples)
- **Initiate-then-put** - Two-step upload (e.g., fal.ai storage API)
- **Data-URI fallback** - When a provider has no `upload` config, the image is inlined as `data:image/png;base64,...` directly in the request body. Used by providers that accept inline URIs (e.g., Meshy).

See [Provider Schema Reference](../guides/PROVIDER_SCHEMA.md#upload-configuration) for configuration details.

## Included Providers

All `providers/*.yaml` files are automatically embedded in the binary at compile time using the `include_dir!` macro. On first run, these configs are written to your user config directory where they can be edited or removed.

**Currently included:**

- **fal.ai** ([`providers/fal-ai.yaml`](../../providers/fal-ai.yaml)) - Text-to-image and image-to-3D models with dynamic discovery. Pay-per-call billing; uses two-step upload (`initiate_then_put`).
- **Meshy AI** ([`providers/meshy.yaml`](../../providers/meshy.yaml)) - Native Meshy API for text-to-image and image-to-3D. Subscription + credits billing; no upload endpoint (uses data-URI inline). Exposes `status_url_template` for task-id-based polling and `cancel_method: DELETE`.

Each YAML file in `providers/` defines models and API configuration. Only files directly in `providers/` are embedded; removing a file excludes its provider from the binary.

## Adding Custom Providers

1. Create a YAML config file in `providers/` and rebuild (embedded in binary)
2. OR create a YAML config file in your user config directory (runtime-only)
3. Set any required environment variables (API keys)
4. Restart the application - your provider appears automatically

For complete configuration instructions and examples, see [Provider Schema Reference](../guides/PROVIDER_SCHEMA.md).

## Provider Loading

The registry loads provider YAML configs in this order:

1. **Embedded providers** (always available, compiled into binary)
2. **Filesystem providers/** (dev mode overrides)
3. **User config directory** (highest priority)

User configs override embedded configs with the same ID.

## Dynamic Model Discovery

Providers can optionally fetch available models dynamically from their API at runtime instead of relying on static YAML model lists.

### How It Works

1. **Configuration**: Provider YAML includes `discovery` section with API endpoint
2. **Production**: Discovery is disabled — apps use curated static models from YAML
3. **Development**: Run `make refresh-models` to discover new models from provider APIs
4. **OpenAPI Parsing**: Fetches OpenAPI 3.0 schemas and auto-generates request/response templates
5. **Caching**: Discovered models cached in-memory with TTL (default: 1 hour)
6. **Fallback**: On discovery failure, falls back to static YAML models

### Discovery Configuration

Providers specify discovery endpoints in their YAML:

```yaml
provider:
  discovery:
    enabled: true
    cache_ttl_secs: 3600        # 1 hour cache
    timeout_secs: 5             # Discovery timeout

    text_to_image:
      endpoint: "https://api.provider.com/models"
      params:
        category: "text-to-image"
      models_field: "models"
      fetch_schemas: true       # Fetch OpenAPI schemas
      field_mapping:
        id_field: "endpoint_id"
        name_field: "display_name"
        openapi_field: "openapi"
```

### OpenAPI Schema Generation

When OpenAPI schemas are available, the system automatically:

- **Parses request schemas** to identify required/optional parameters
- **Maps known fields** to template variables (`prompt` → `${prompt}`, `image_url` → `${image_url}`)
- **Extracts defaults** from schema for optional parameters
- **Generates response templates** (defaults to polling pattern)

This eliminates manual template creation for providers with large or frequently-changing model catalogs.

### Discovery Cache

- **In-memory storage**: Cache persists during app lifetime (GUI) or single invocation (CLI)
- **TTL-based expiry**: Models automatically refresh after cache TTL (default: 1 hour)
- **Manual refresh**: `make refresh-models` for development use
- **Keyed by capability**: Text-to-image and image-to-3D models cached separately

### Development Discovery

Discovery is a dev-only tool for evaluating newly available models:

```bash
# List providers with static curated models (instant)
asset-tap --list-providers

# Discover new models from provider APIs (dev only, 2-5s)
make refresh-models
```

End users see only the curated static models from provider YAML files.

### Benefits

- **Always up-to-date**: Automatically reflects new models added by provider
- **Zero maintenance**: No manual YAML updates when provider adds/removes models
- **Graceful degradation**: Falls back to static models on network failure
- **Fast iteration**: Developers can test new models without code changes

### Implementation

- **Discovery client**: `core/src/providers/discovery.rs`
- **OpenAPI parser**: `core/src/providers/openapi.rs`
- **Cache**: `core/src/providers/discovery_cache.rs`
- **Registry integration**: `core/src/providers/registry.rs`

See [`providers/fal-ai.yaml`](../../providers/fal-ai.yaml) for a full example using `initiate_then_put` upload and queue-based polling, or [`providers/meshy.yaml`](../../providers/meshy.yaml) for the task-id polling / data-URI / DELETE-cancel pattern.

## Variable Interpolation

Templates support variable substitution:

- `${prompt}` - User's prompt
- `${image_url}` - Uploaded image URL
- `${API_KEY}` - Environment variable value
- `${ANY_ENV_VAR}` - Any environment variable

## CLI Usage

```bash
# List all providers with static models (instant)
asset-tap --list-providers

# Discover new models from provider APIs (dev only)
make refresh-models

# Use specific provider
asset-tap -p <provider-id> -y "a dragon"

# Use provider with specific models
asset-tap -p fal.ai --image-model fal-ai/nano-banana-2 -y "a robot"

# Use native Meshy end-to-end
asset-tap -p meshy --image-model meshy/nano-banana-pro --3d-model meshy/v6/image-to-3d -y "a robot"
```

## GUI Usage

1. Open the Provider dropdown
2. Select your provider
3. Model dropdowns update automatically
4. Generate as usual

## Benefits

- **Zero-code extensibility**: Add providers via YAML
- **Hermetic**: Each provider is self-contained
- **Community sharing**: Share YAML configs
- **Auto-detection**: Intelligent upload handling
- **Type-safe**: Validated at load time

## See Also

- **[Provider Schema Reference](../guides/PROVIDER_SCHEMA.md)** - Complete YAML configuration guide
- **Example configs**: See `providers/*.yaml` for working examples
- **Implementation**: `core/src/providers/registry.rs` (provider loading), `core/src/providers/http_client.rs` (HTTP execution)
