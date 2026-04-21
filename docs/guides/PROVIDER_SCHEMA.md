# Provider Configuration Schema Reference

Complete reference for creating and configuring AI provider plugins via YAML configuration files.

## Overview

Providers are defined through YAML configuration files that specify:

- Provider metadata (ID, name, URLs)
- Authentication requirements
- Available models and endpoints
- Request/response templates
- Upload configuration (if needed)

## Schema Structure

### Top Level

<!-- dprint-ignore -->
```yaml
provider:           # Required: Provider metadata
  # ... metadata fields
  upload:           # Optional: File upload configuration (nested under provider)
    # ... upload config

text_to_image:     # Optional: Text-to-image models
  # ... model configs

image_to_3d:       # Optional: Image-to-3D models
  # ... model configs
```

**Embedded config sync:** On app startup, each embedded provider YAML is compared byte-for-byte against its on-disk copy. If they differ, the on-disk file is backed up as `.yaml.bak` and overwritten with the embedded version. There is no manual version field — the content itself is the version. User-created custom YAML files (with filenames that don't match any embedded file) are never touched.

## Provider Metadata

**Required fields:**

```yaml
provider:
  id: string # Unique identifier (e.g., "my-provider")
  name: string # Display name (e.g., "My Provider")
  description: string # Provider description
  env_vars: [string] # Required environment variables (e.g., ["FAL_KEY"])
```

**Optional fields:**

```yaml
base_url: string # Base URL for API endpoints (e.g., "https://api.example.com")
api_key_url: string # URL where users can obtain API keys
website_url: string # Provider's main website
docs_url: string # Link to provider's API documentation
discovery: object # Optional: Dynamic model discovery configuration (see Discovery section)
```

**Guidelines:**

- `id`: Use lowercase with dots/hyphens (e.g., "fal.ai", "my-provider")
- `name`: Human-readable display name
- `env_vars`: List all required environment variables for authentication
- `base_url`: Should NOT include trailing slash
- URL fields: Must be valid HTTPS URLs to official provider resources

## Model Configuration

Models are defined in either `text_to_image` or `image_to_3d` arrays:

<!-- dprint-ignore -->
```yaml
text_to_image:
  - id: string                 # Unique model ID within provider
    name: string               # Display name
    description: string        # Model description
    endpoint: string           # API endpoint (relative or absolute)
    method: string             # HTTP method (POST, GET, etc.) - defaults to POST
    request:                   # Request template
      # ... request config
    response:                  # Response template
      # ... response config
```

**Guidelines:**

- `id`: Fully-qualified, lowercase with hyphens, namespaced by provider (e.g., "fal-ai/nano-banana-2", "fal-ai/trellis-2", "meshy/v6/image-to-3d")
- `endpoint`: Relative to `base_url` (e.g., "/predictions") or absolute URL
- Set `is_default: true` on the model that should be the default; if none specified, the first model in the array is used as fallback

## Request Templates

### Basic JSON Request

```yaml
request:
  headers:
    Authorization: 'Bearer ${API_KEY}'
    Content-Type: 'application/json'
  body:
    prompt: '${prompt}' # Use ${prompt} for text-to-image
    model_id: 'specific-model'
    num_outputs: 1
```

### Multipart File Upload

For models that accept direct file uploads:

```yaml
request:
  headers:
    Authorization: 'Key ${API_KEY}'
  multipart:
    file_field: 'image' # Name of the file field
    fields: # Additional form fields
      seed: '42'
      quality: 'high'
```

### URL-Based Input

For models that require a public URL:

```yaml
request:
  headers:
    Authorization: 'Token ${API_KEY}'
    Content-Type: 'application/json'
  body:
    image_url: '${image_url}' # System uploads file and provides URL
    prompt: 'optional prompt'
```

**Variable Interpolation:**

- `${prompt}` - User's text prompt (text-to-image models)
- `${image_url}` - Auto-generated public URL for uploaded image
- `${ENV_VAR}` - Any environment variable defined in `env_vars`

## Response Templates

### Response Types

#### Json

Extract a URL from JSON response and download the content:

```yaml
response:
  response_type: Json
  field: 'images[0].url' # JSONPath to the result URL
```

**JSONPath examples:**

- `"image_url"` - Top-level field
- `"data.images[0].url"` - Nested with array
- `"output"` - Direct field access

#### Binary

Direct binary response (image/model data):

```yaml
response:
  response_type: Binary
```

Use when API returns raw file data directly.

#### Base64

Decode base64-encoded data from JSON:

```yaml
response:
  response_type: Base64
  field: 'artifacts[0].base64' # JSONPath to base64 string
```

#### Polling

For asynchronous APIs that require status polling:

```yaml
response:
  response_type: polling
  polling:
    status_field: 'status_url' # Field from initial response used to construct poll URL
    status_url_template: '/v1/jobs/${status_field}' # (Optional) template-built poll URL — see below
    status_check_field: 'status' # Field to check in polling response
    success_value: 'COMPLETED' # Value indicating completion
    failure_value: 'FAILED' # Value indicating failure
    result_field: 'images[0].url' # JSONPath to final result URL
    interval_ms: 1000 # Poll every 1 second
    max_attempts: 300 # Maximum 300 attempts
    response_url_field: 'response_url' # (Optional) Field with URL to fetch final result
    response_envelope_field: 'response' # (Optional) Envelope field wrapping the output
    poll_query_params: '?logs=1' # (Optional) Query params appended to poll URL
    cancel_url_template: '${status_url}/cancel' # (Optional) URL template for cancelling
```

**Polling workflow:**

1. Initial request returns job ID (extracted via `status_field`)
2. System polls `GET {base_url}/{endpoint}/{job_id}` every `interval_ms`
3. Checks `status_check_field` until it equals `success_value` or `failure_value`
4. On success, extracts result from `result_field`

**`status_url_template`** (optional) — for providers that return only a task id
instead of a full status URL (e.g. Meshy's `{"result": "<task-id>"}`). When set,
the poll URL is built by substituting `${field}` tokens against the initial
response JSON. Nested paths (`${data.id}`) and array indices (`${items[0]}`)
are supported. Relative paths are resolved against `provider.base_url`.

```yaml
# Meshy example: initial response is {"result": "abc-123"}
polling:
  status_field: 'result'
  status_url_template: '/openapi/v1/image-to-3d/${result}'
  # → polls https://api.meshy.ai/openapi/v1/image-to-3d/abc-123
```

**Guidelines:**

- `interval_ms`: Balance between responsiveness and API rate limits
  - Fast operations (images): 1000ms (1 second)
  - Slow operations (3D models): 2000ms (2 seconds)
- `max_attempts`: Set based on typical operation time
  - Images: 120 attempts (2 minutes at 1s intervals)
  - 3D models: 180-300 attempts (6-10 minutes at 2s intervals)

## Dynamic Model Discovery

Dynamic discovery allows providers to automatically fetch available models from their API at runtime, eliminating the need to hardcode model lists in YAML. Models are discovered with OpenAPI schema parsing to automatically generate request/response templates.

### Discovery Configuration

<!-- dprint-ignore -->
```yaml
provider:
  discovery:
    enabled: boolean              # Enable/disable discovery
    cache_ttl_secs: number        # Cache duration (default: 3600 = 1 hour)
    require_auth: boolean         # Whether discovery endpoint needs auth (default: false)
    timeout_secs: number          # Discovery request timeout (default: 5)

    text_to_image:                # Discovery config for text-to-image capability
      endpoint: string            # API endpoint returning model list
      params:                     # Query parameters (optional)
        key: "value"
      models_field: string        # JSONPath to models array (e.g., "models")
      fetch_schemas: boolean      # Fetch OpenAPI schemas for auto-generation
      schema_expand_param: string # Query param to enable schema expansion (optional)
      field_mapping:              # How to extract model data from API response
        id_field: string          # Model ID field (e.g., "endpoint_id")
        name_field: string        # Model name field (e.g., "metadata.display_name")
        description_field: string # Model description field (optional)
        endpoint_field: string    # Endpoint path field (optional)
        status_field: string      # Status field for filtering (optional)
        active_status_value: string # Value indicating active models (e.g., "active")
        openapi_field: string     # Field containing OpenAPI schema (optional)

    image_to_3d:                  # Same structure for image-to-3D capability
      # ... (same fields as text_to_image)
```

### Field Mapping

The `field_mapping` object supports JSONPath-like syntax for nested fields:

- **Simple fields**: `"id"`, `"name"`, `"status"`
- **Nested fields**: `"metadata.display_name"`, `"config.endpoint"`
- **Arrays**: Use dot notation for object arrays (not array indices)

**Example API response:**

```json
{
  "models": [
    {
      "endpoint_id": "fal-ai/flux/dev",
      "metadata": {
        "display_name": "FLUX.1 [dev]",
        "description": "Fast image generation",
        "status": "active"
      },
      "openapi": {/* OpenAPI 3.0 schema */}
    }
  ]
}
```

**Corresponding field mapping:**

```yaml
field_mapping:
  id_field: 'endpoint_id'
  name_field: 'metadata.display_name'
  description_field: 'metadata.description'
  status_field: 'metadata.status'
  active_status_value: 'active'
  openapi_field: 'openapi'
```

### OpenAPI Schema Parsing

When `fetch_schemas: true` and `openapi_field` is provided, the system automatically:

1. **Parses OpenAPI 3.0 schemas** from the discovery response
2. **Generates request templates** by mapping known fields:
   - `prompt`, `text`, `description` → `${prompt}`
   - `image`, `image_url`, `input_image` → `${image_url}`
   - Other fields with defaults → use schema defaults
3. **Generates response templates** (defaults to polling pattern)
4. **Falls back gracefully** if parsing fails (uses basic template)

**Benefits:**

- No manual template creation for discovered models
- Automatically adapts to provider API changes
- Supports providers with large/changing model catalogs

### Discovery Behavior

Discovery is a **development-only** tool for evaluating newly available models. End users see only the curated static models from provider YAML files.

**Production (GUI + CLI):**

- Uses curated static models from YAML (instant, no API calls)
- Discovery is disabled — apps never contact provider discovery APIs

**Development:**

- Run `make refresh-models` to discover new models from provider APIs (2-5s)
- Discovered models cached in memory (TTL: 1 hour default)
- Failed discovery falls back to static YAML models

### Development Commands

```bash
# List providers with static curated models (instant)
asset-tap --list-providers

# Discover new models from provider APIs (dev only, 2-5s)
make refresh-models
```

### Example: fal.ai Discovery

See `providers/fal-ai.yaml` for a complete working example of dynamic discovery with OpenAPI schema parsing.

## Upload Configuration

Required when models need public URLs for image inputs (detected by `${image_url}` in request). The `upload` section is nested under `provider:`.

### Single-Step Multipart Upload

```yaml
provider:
  # ... other provider fields ...
  upload:
    endpoint: '/files'
    method: POST
    request:
      type: multipart
      file_field: 'content'
      headers:
        Authorization: 'Token ${API_KEY}'
    response:
      file_url_field: 'urls.get' # JSONPath to public URL
```

### Two-Step Initiate-Then-Put

For providers using pre-signed URLs:

```yaml
provider:
  # ... other provider fields ...
  upload:
    endpoint: '/storage/upload/initiate'
    method: POST
    request:
      type: initiate_then_put
      headers:
        Authorization: 'Key ${API_KEY}'
        Content-Type: 'application/json'
      initiate_body:
        file_name: 'image.png'
        content_type: 'image/png'
    response:
      upload_url_field: 'upload_url' # Pre-signed PUT URL
      file_url_field: 'file_url' # Final public URL
```

**Workflow:**

1. POST to `endpoint` with `initiate_body` to get upload URL
2. PUT file data to `upload_url`
3. Use `file_url` as `${image_url}` in model request

**Example:** fal.ai storage API

### Data-URI Fallback (no upload endpoint)

When a provider omits `upload` entirely, the pipeline inlines the image as a
`data:image/png;base64,...` URI wherever `${image_url}` appears in the model
request body. Used for providers that accept inline data URIs directly and do
not expose an upload endpoint (e.g. Meshy).

No YAML configuration is required — simply omit the `upload` block from your
provider. The pipeline enforces a 10 MB cap on the raw image bytes in this
mode to prevent request-size failures on providers with body limits.

**Example:** Meshy image-to-3D

## Complete Examples

### Example 1: Simple Polling API (fal.ai nano-banana-2)

```yaml
provider:
  id: 'fal.ai'
  name: 'fal.ai'
  description: 'Fast, serverless AI model API'
  env_vars: ['FAL_KEY']
  base_url: 'https://queue.fal.run'
  api_key_url: 'https://fal.ai/dashboard/keys'
  website_url: 'https://fal.ai'
  docs_url: 'https://docs.fal.ai/model-apis'

text_to_image:
  - id: 'fal-ai/nano-banana-2'
    name: 'Nano Banana 2'
    description: 'Gemini 3.1 Flash Image — reasoning-guided generation'
    endpoint: '/fal-ai/nano-banana-2'
    method: POST
    request:
      headers:
        Authorization: 'Key ${FAL_KEY}'
        Content-Type: 'application/json'
      body:
        prompt: '${prompt}'
        resolution: '1K'
        num_images: 1
    response:
      response_type: polling
      polling:
        status_field: 'status_url'
        status_check_field: 'status'
        success_value: 'COMPLETED'
        failure_value: 'FAILED'
        result_field: 'images[0].url'
        interval_ms: 1000
        max_attempts: 120
```

### Example 2: Dynamic Model Discovery (fal.ai)

```yaml
provider:
  id: 'fal.ai'
  name: 'fal.ai'
  description: 'Fast, serverless AI model API'
  env_vars: ['FAL_KEY']
  base_url: 'https://queue.fal.run'
  api_key_url: 'https://fal.ai/dashboard/keys'

  # Upload configuration (initiate-then-put pattern)
  upload:
    endpoint: 'https://rest.alpha.fal.ai/storage/upload/initiate?storage_type=fal-cdn-v3'
    method: POST
    request:
      type: initiate_then_put
      headers:
        Authorization: 'Key ${FAL_KEY}'
        Content-Type: 'application/json'
      initiate_body:
        file_name: 'image.png'
        content_type: 'image/png'
    response:
      upload_url_field: 'upload_url'
      file_url_field: 'file_url'

  # Dynamic model discovery configuration
  discovery:
    enabled: true
    cache_ttl_secs: 3600
    require_auth: false
    timeout_secs: 5

    text_to_image:
      endpoint: 'https://api.fal.ai/v1/models'
      params:
        category: 'text-to-image'
        limit: '10'
      models_field: 'models'
      fetch_schemas: true
      schema_expand_param: 'expand'
      field_mapping:
        id_field: 'endpoint_id'
        name_field: 'metadata.display_name'
        description_field: 'metadata.description'
        endpoint_field: 'endpoint_id'
        status_field: 'metadata.status'
        active_status_value: 'active'
        openapi_field: 'openapi'

# Static fallback models (used until discovery runs)
text_to_image:
  - id: 'fal-ai/nano-banana-2'
    name: 'Nano Banana 2'
    description: 'Gemini 3.1 Flash Image — reasoning-guided generation'
    endpoint: '/fal-ai/nano-banana-2'
    method: POST
    request:
      headers:
        Authorization: 'Key ${FAL_KEY}'
        Content-Type: 'application/json'
      body:
        prompt: '${prompt}'
        resolution: '1K'
    response:
      response_type: polling
      polling:
        status_field: 'status_url'
        result_field: 'images[0].url'
        status_check_field: 'status'
        success_value: 'COMPLETED'
        failure_value: 'FAILED'
        interval_ms: 1000
        max_attempts: 120
```

### Example 3: Custom Provider Pattern

For additional provider examples, see the working configs in `providers/` directory:

- **fal.ai** (`providers/fal-ai.yaml`) - Dynamic discovery with initiate-then-put upload

See the existing configs in `providers/` for reference.

## Best Practices

### URLs and Endpoints

1. **Always use official URLs** - Verify against provider documentation
2. **No trailing slashes** in `base_url`
3. **Relative paths** for `endpoint` when possible (e.g., "/task" not "https://api.example.com/task")
4. **HTTPS only** - HTTP is not supported

### Model Identifiers

1. **Provider-specific IDs** - Use official model names/IDs from provider docs
2. **Consistent naming** - Use the same ID format across all your configs
3. **Version strings** - Include full version IDs when required by the provider

### Response Field Paths

1. **Verify JSONPath** - Test against actual API responses
2. **Array notation** - Use `[0]` for first element, not `[1]`
3. **Nested fields** - Use dot notation: `data.output.url`
4. **Root fields** - No leading dot: `image_url` not `.image_url`

### Authentication

1. **Environment variables** - Never hardcode API keys
2. **Correct header format** - Check provider docs for exact format
   - `"Bearer ${KEY}"` vs `"Token ${KEY}"` vs `"Key ${KEY}"`
3. **Header names** - Case-sensitive: `Authorization` not `authorization`

### Polling Configuration

1. **Reasonable intervals** - Don't poll too frequently (respect rate limits)
2. **Adequate timeouts** - Set `max_attempts` based on typical operation time
3. **Failure handling** - Always specify `failure_value` if provider supports it

## Validation

Provider configs are validated on load. Common errors:

- **Missing required fields** - Ensure all required fields are present
- **Invalid URLs** - Must be valid HTTPS URLs
- **Invalid JSONPath** - Field paths must match response structure
- **Unconfigured environment variables** - Provider won't load if env vars missing
- **Incorrect response type** - Must match actual API response format

## Testing Your Provider

1. **Check provider loads**:
   ```bash
   cargo run --bin asset-tap -- --list-providers
   ```

2. **Verify environment variables** - Provider won't appear if env vars missing

3. **Use mock mode** to verify config parsing and pipeline plumbing (no API costs):
   ```bash
   make mock ARGS='-p your-provider -y "test"'
   # Or build with the feature explicitly:
   cargo run --features mock --bin asset-tap -- --mock -p your-provider -y "test"
   ```
   Mock mode requires the `mock` Cargo feature (not included in release builds). It returns generic synthetic responses — it validates that your YAML loads and the pipeline runs, but does not test your provider's actual response format.

4. **Test with real API** to validate response field extraction — start with text-to-image (faster/cheaper)

## Adding Your Provider

**To add a provider to the default embedded set:**

1. Create your YAML config file in `providers/your-provider.yaml`
2. Run `make build` (or `cargo build --release`)
   - The `include_dir!` macro automatically discovers and embeds all `*.yaml` files
   - No code changes needed!
3. Verify provider loads: `cargo run --bin asset-tap -- --list-providers`
4. Test in mock mode to confirm config parsing: `make mock ARGS='-p your-provider -y "test"'`
5. Set required environment variables and test with real API to validate response parsing

**To add a provider for personal use only (not embedded):**

1. Create your YAML config in your user config directory:
   - Dev mode: `.dev/providers/your-provider.yaml`
   - Release (macOS): `~/Library/Application Support/asset-tap/providers/your-provider.yaml`
   - Release (Linux): `~/.config/asset-tap/providers/your-provider.yaml`
2. Set required environment variables
3. Restart the application

**To remove a provider (exclude from embedding):**

Delete or move its YAML file out of the `providers/` directory and rebuild.

## Reference

- Provider system architecture: [docs/architecture/PROVIDERS.md](../architecture/PROVIDERS.md)
- Rust schema definitions: [core/src/providers/config.rs](../../core/src/providers/config.rs)
- Example configs: [providers/](../../providers/)
