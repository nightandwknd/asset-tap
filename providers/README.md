# Custom Provider Configurations

This directory contains YAML configuration files for custom AI provider integrations.

## Quick Start

1. Copy [fal-ai.yaml](fal-ai.yaml) and rename it to your provider name
2. Update the provider metadata and API endpoint configuration
3. Set the required environment variables (e.g., `MY_API_KEY`)
4. Restart the application - your provider will be automatically loaded

## Configuration Format

### Provider Metadata

```yaml
provider:
  id: "my-provider"           # Unique identifier
  name: "My Provider"         # Display name
  description: "Description"  # Provider description
  env_vars:                   # Required environment variables
    - "MY_API_KEY"
  base_url: "https://..."     # Optional base URL
```

### Model Configuration

Each model requires:

- **id**: Unique model identifier
- **name**: Display name
- **description**: Model description
- **cost_per_run**: Approximate cost per run as a float (e.g., `0.05`)
- **endpoint**: API endpoint path
- **method**: HTTP method (GET, POST, PUT, DELETE, PATCH)
- **request**: Request configuration
- **response**: Response parsing configuration

### Request Templates

#### JSON Request

```yaml
request:
  headers:
    Authorization: "Bearer ${API_KEY}"
    Content-Type: "application/json"
  body:
    prompt: "${prompt}"
    model: "my-model-v1"
```

#### Multipart Request (for file uploads)

```yaml
request:
  multipart:
    file_field: "image"
    fields:
      model: "my-3d-model"
  headers:
    Authorization: "Bearer ${API_KEY}"
```

### Response Types

#### 1. JSON (URL extraction)

Downloads from a URL in the JSON response:

```yaml
response:
  response_type: Json
  field: "data.url"  # JSONPath to URL field
```

#### 2. Binary

Direct binary response:

```yaml
response:
  response_type: Binary
```

#### 3. Base64

Base64-encoded data in JSON:

```yaml
response:
  response_type: Base64
  field: "data.image"
```

#### 4. Polling

For async operations that require polling:

```yaml
response:
  response_type: Polling
  polling:
    status_field: "job_id"              # Initial response field with job ID (or full status URL)
    status_url_template: "/v1/jobs/${job_id}"  # Optional: build the poll URL from initial response fields
    status_check_field: "status"        # Status field in poll response
    success_value: "completed"          # Value indicating success
    failure_value: "failed"             # Value indicating failure (optional)
    result_field: "result.model_url"    # Field containing result URL
    interval_ms: 2000                   # Poll interval in milliseconds
    max_attempts: 60                    # Max polling attempts
```

## Variable Interpolation

Use `${VAR}` syntax to insert:

- Environment variables (e.g., `${MY_API_KEY}`)
- Request parameters (e.g., `${prompt}`)

## File Formats

Only YAML format is supported:

- `.yaml` or `.yml`

## Examples

See [fal-ai.yaml](fal-ai.yaml) for a complete example with:

- Text-to-image models (multiple quality tiers)
- Image-to-3D models (with polling)
- All response type patterns

See [meshy.yaml](meshy.yaml) for a provider that uses:

- `status_url_template` — polling URL built from a task id (for APIs that return `{"result": "<id>"}` instead of a full status URL)
- Data-URI image input — no upload endpoint configured; the pipeline inlines the image as `data:image/png;base64,...` automatically

## Notes

- Providers are loaded at startup
- Invalid configs are logged but don't crash the app
- Set `is_default: true` on a model to make it the default selection; if no model has this flag, the first model is used
- Config changes require app restart

## Additional Model Fields

Models support additional optional fields beyond the basics:

- **`is_default`** (`bool`) - Mark this model as the default selection for its capability. Only one model per capability should have this set to `true`.
- **`auth_format`** (`string`) - Override the authentication header format for this model (e.g., `"Bearer ${FAL_KEY}"`, `"Key ${FAL_KEY}"`).
- **`cost_per_run`** (`f64`) - Approximate cost per generation run displayed to the user (e.g., `0.05`).
- **`poll_query_params`** (`string`) - Additional query parameters to append to each polling status check URL (e.g., `"?logs=1"`).
- **`cancel_url_template`** (`string`) - URL template for cancelling in-progress requests (e.g., `"https://queue.fal.run/{model_id}/requests/{request_id}/cancel"`).
