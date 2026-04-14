+++
title = "Providers"
description = "Included AI providers, available models, and how to add your own custom providers."
date = 2026-02-10
weight = 5
in_search_index = true

[extra]
images = []

[taxonomies]
tags = ["reference", "providers"]
+++

Asset Tap uses a data-driven provider system where AI providers are defined entirely through YAML configuration files. No code changes are needed to add, remove, or modify providers.

## Included Providers

Asset Tap ships with pre-configured support for two providers. You only need an API key for one of them to run the full pipeline.

- **[fal.ai](https://fal.ai)** -- Pay-per-generation pricing, broadest model selection.
- **[Meshy AI](https://www.meshy.ai)** -- Subscription-based, credit pool; specialized in 3D.

### fal.ai

#### Text-to-Image Models

| Model                                                           | Description                                                         |
| --------------------------------------------------------------- | ------------------------------------------------------------------- |
| [Nano Banana 2](https://fal.ai/models/fal-ai/nano-banana-2)     | Gemini 3.1 Flash Image -- reasoning-guided generation _(default)_   |
| [Nano Banana](https://fal.ai/models/fal-ai/nano-banana)         | Google Imagen 3-based image generation -- fast and affordable       |
| [Nano Banana Pro](https://fal.ai/models/fal-ai/nano-banana-pro) | Premium Google Imagen 3 -- higher quality with aspect ratio control |
| [FLUX.2 Dev](https://fal.ai/models/fal-ai/flux-2)               | Open-source FLUX.2 with tunable guidance and steps                  |
| [FLUX.2 Pro](https://fal.ai/models/fal-ai/flux-2-pro)           | Premium FLUX.2 -- best quality, zero-config                         |

#### Image-to-3D Models

| Model                                                                         | Description                                                  |
| ----------------------------------------------------------------------------- | ------------------------------------------------------------ |
| [TRELLIS 2](https://fal.ai/models/fal-ai/trellis-2)                           | Native 3D generative model -- fast and versatile _(default)_ |
| [Hunyuan3D Pro](https://fal.ai/models/fal-ai/hunyuan-3d/v3.1/pro/image-to-3d) | Tencent Hunyuan3D v3.1 Pro -- high quality 3D generation     |
| [Meshy v6](https://fal.ai/models/fal-ai/meshy/v6/image-to-3d)                 | Meshy 6 proxied through fal -- pay-per-call billing          |

> **Tip:** Your [fal.ai Dashboard](https://fal.ai/dashboard/recent-history) shows all generation requests, results, and costs. This is the source of truth for your usage and a handy way to recover past outputs.

### Meshy AI

Native Meshy API -- bypasses fal's proxy markup and unlocks the full Meshy feature set. Requires `MESHY_API_KEY` from the [Meshy API settings page](https://www.meshy.ai/settings/api).

#### Text-to-Image Models

| Model                                                         | Description                                     |
| ------------------------------------------------------------- | ----------------------------------------------- |
| [Nano Banana](https://docs.meshy.ai/en/api/text-to-image)     | Meshy's standard text-to-image tier _(default)_ |
| [Nano Banana Pro](https://docs.meshy.ai/en/api/text-to-image) | Higher-quality text-to-image tier               |

Tunable parameters: `aspect_ratio` (1:1, 16:9, 9:16, 4:3, 3:4), `generate_multi_view`.

#### Image-to-3D Models

| Model                                                | Description                                                  |
| ---------------------------------------------------- | ------------------------------------------------------------ |
| [Meshy v6](https://docs.meshy.ai/en/api/image-to-3d) | Meshy 6 -- production-ready 3D with PBR textures _(default)_ |
| [Meshy v5](https://docs.meshy.ai/en/api/image-to-3d) | Previous generation, lower credit cost                       |

Tunable parameters: `topology` (triangle/quad), `target_polycount`, `enable_pbr`, `should_remesh`, `should_texture`.

> **Why two ways to reach Meshy?** The fal.ai "Meshy v6" entry uses fal's pay-per-call billing and requires a `FAL_KEY`. The Meshy provider's entry uses Meshy's subscription credits and requires a `MESHY_API_KEY`. Pick whichever fits your billing relationship -- or keep both keys configured and switch per generation.

### Pricing Models

| Provider | Billing      | How it works                                                                |
| -------- | ------------ | --------------------------------------------------------------------------- |
| fal.ai   | Pay-per-call | Charged per generation at the model's listed cost; no monthly minimum.      |
| Meshy AI | Subscription | Monthly plan grants a credit pool; each generation deducts credits from it. |

Meshy credit costs (verified 2026-04): v5 image-to-3D is 5 credits (15 with textures); v6 is 20 credits (30 with textures). See the [Meshy pricing page](https://www.meshy.ai/pricing) for plan details.

---

## Adding Custom Providers

You can add support for any AI provider by creating a YAML configuration file. No code changes required.

### Quick Start

Create a YAML file with your provider's API details:

```yaml
provider:
  id: "my-provider"
  name: "My Provider"
  description: "Custom AI provider"
  env_vars: ["MY_API_KEY"]
  base_url: "https://api.example.com"
  api_key_url: "https://example.com/keys"

text_to_image:
  - id: "my-model"
    name: "My Model"
    description: "Fast image generation"
    endpoint: "/generate"
    method: POST
    request:
      headers:
        Authorization: "Bearer ${MY_API_KEY}"
        Content-Type: "application/json"
      body:
        prompt: "${prompt}"
    response:
      response_type: Json
      field: "image_url"
```

### Where to Put Your Config

**For personal use (no rebuild needed):**

Place the YAML file in your user config directory:

- **macOS**: `~/Library/Application Support/asset-tap/providers/my-provider.yaml`
- **Linux**: `~/.config/asset-tap/providers/my-provider.yaml`
- **Windows**: `%APPDATA%/asset-tap/providers/my-provider.yaml`

Restart the application and your provider will appear automatically.

**To embed in the binary (requires rebuild):**

1. Add the file to `providers/my-provider.yaml` in the source tree
2. Run `make build` -- the `include_dir!` macro discovers all `*.yaml` files automatically

### Authentication

List required environment variables in `env_vars`. The provider won't appear as available until all are set.

```yaml
provider:
  env_vars: ["MY_API_KEY", "MY_SECRET"]
```

Use `${ENV_VAR}` syntax in request templates:

```yaml
request:
  headers:
    Authorization: "Bearer ${MY_API_KEY}"
```

In the GUI, set API keys in Settings. For the CLI, use environment variables or a `.env` file.

### Response Types

**JSON** -- Extract a URL from a JSON response:

```yaml
response:
  response_type: Json
  field: "data.images[0].url"    # JSONPath expression
```

**Polling** -- For async APIs that queue jobs:

```yaml
response:
  response_type: polling
  polling:
    status_field: "status_url"        # Field in submit response containing the status check URL
    status_check_field: "status"      # Field in status response to check
    success_value: "COMPLETED"
    failure_value: "FAILED"
    response_url_field: "response_url"  # Field containing URL to fetch final result
    response_envelope_field: "response" # Field in result that wraps the actual output
    result_field: "images[0].url"       # JSONPath to extract from the output
    interval_ms: 1000
    max_attempts: 120

    # Optional: build the poll URL from a task id instead of reading a full URL
    # from the initial response. Used when the API returns only {"result": "<id>"}.
    status_url_template: "/v1/jobs/${result}"

    # Optional: override the cancel HTTP method. Defaults to PUT.
    # Meshy uses DELETE for its cancel endpoint.
    cancel_method: DELETE
    cancel_url_template: "${status_url}"
```

`status_url_template` supports nested paths (`${data.id}`) and array indices (`${items[0]}`). Relative paths are resolved against `base_url`.

**Binary / Base64** -- For direct file responses:

```yaml
response:
  response_type: Binary

# or base64-encoded in JSON:
response:
  response_type: Base64
  field: "artifacts[0].base64"
```

### Image-to-3D Models

Image-to-3D models use `${image_url}` instead of `${prompt}`. Asset Tap automatically uploads the image and substitutes the public URL:

```yaml
image_to_3d:
  - id: "my-3d-model"
    name: "My 3D Model"
    endpoint: "/3d/generate"
    method: POST
    request:
      headers:
        Authorization: "Key ${MY_API_KEY}"
        Content-Type: "application/json"
      body:
        image_url: "${image_url}"
    response:
      response_type: polling
      polling:
        status_field: "id"
        status_check_field: "status"
        success_value: "succeeded"
        result_field: "model_glb.url"
        interval_ms: 2000
        max_attempts: 300
```

### Upload Configuration

Required when models use `${image_url}` **and** the provider exposes an upload endpoint. The `upload` section is nested under `provider:`:

```yaml
provider:
  id: "my-provider"
  # ... other provider fields ...
  upload:
    endpoint: "/storage/upload/initiate"
    method: POST
    request:
      type: initiate_then_put    # or "multipart"
      headers:
        Authorization: "Key ${MY_API_KEY}"
        Content-Type: "application/json"
      initiate_body:
        file_name: "image.png"
        content_type: "image/png"
    response:
      upload_url_field: "upload_url"
      file_url_field: "file_url"
```

### Data-URI Fallback (No Upload Endpoint)

If a provider doesn't offer an upload endpoint but accepts inline `data:image/png;base64,...` URIs directly (like [Meshy](https://docs.meshy.ai/en/api/image-to-3d)), simply **omit the `upload:` block** from your YAML. Asset Tap automatically inlines the image as a base64 data URI wherever `${image_url}` appears in the request body.

A 10 MB cap on the raw image bytes is enforced in this mode to prevent request-size failures on providers with body limits. For typical Asset Tap workflows (where the intermediate image is 1-4 MB), this is well within limits.

### Testing Your Provider

```bash
# 1. Verify the provider loads and config is valid
asset-tap --list-providers

# 2. Verify it works in mock mode (confirms config parsing)
# Mock mode requires building from source with the mock feature:
make mock ARGS='-p my-provider -y "test prompt"'
# Or: cargo run --features mock --bin asset-tap -- --mock -p my-provider -y "test prompt"

# 3. Test with real API (required to validate response parsing)
asset-tap -p my-provider -y "a red cube"
```

**Note:** Mock mode is a development feature (not available in release builds). It verifies that your provider **loads and configures correctly** (YAML parsing, model registration, endpoint routing), but returns generic synthetic responses. To validate that response field extraction works with your provider's actual API, test against the real API.

---

## Schema Reference

Complete reference for all provider YAML fields.

### Top-Level Structure

```yaml
provider:           # Required: Provider metadata
  id: string
  name: string
  description: string
  env_vars: [string]
  base_url: string          # Optional
  api_key_url: string       # Optional
  website_url: string       # Optional
  docs_url: string          # Optional
  upload:                   # Optional: File upload configuration (nested under provider)

text_to_image:      # Optional: Text-to-image model list
  - id: string

image_to_3d:        # Optional: Image-to-3D model list
  - id: string
```

### Model Fields

```yaml
text_to_image:     # or image_to_3d
  - id: string           # Unique model ID within provider
    name: string         # Display name
    description: string  # Model description
    endpoint: string     # API endpoint (relative to base_url or absolute)
    method: string       # HTTP method (default: POST)
    request:
      headers: {}        # HTTP headers with ${VAR} interpolation
      body: {}           # JSON body with ${prompt} or ${image_url}
    response:
      response_type: string   # Json, Binary, Base64, or polling
      field: string            # JSONPath for Json/Base64
      polling:                 # Required for polling type
        status_field: string
        status_url_template: string  # Optional: build poll URL from initial response
        status_check_field: string
        success_value: string
        failure_value: string
        result_field: string
        interval_ms: integer
        max_attempts: integer
        cancel_method: string    # Optional: HTTP method for cancel (default PUT)
        cancel_url_template: string  # Optional: template using ${status_url}
```

### Variable Interpolation

- `${prompt}` -- User's text prompt
- `${image_url}` -- Publicly accessible URL for the generated image. Produced by the provider's `upload` endpoint if configured, otherwise inlined as a `data:image/png;base64,...` URI.
- `${ENV_VAR}` -- Any environment variable listed in `env_vars`

### Complete Example

```yaml
provider:
  id: "fal.ai"
  name: "fal.ai"
  description: "Fast, serverless AI model API"
  env_vars: ["FAL_KEY"]
  base_url: "https://queue.fal.run"
  api_key_url: "https://fal.ai/dashboard/keys"
  upload:
    endpoint: "https://rest.alpha.fal.ai/storage/upload/initiate?storage_type=fal-cdn-v3"
    method: POST
    request:
      type: initiate_then_put
      headers:
        Authorization: "Key ${FAL_KEY}"
        Content-Type: "application/json"
      initiate_body:
        file_name: "image.png"
        content_type: "image/png"
    response:
      upload_url_field: "upload_url"
      file_url_field: "file_url"

text_to_image:
  - id: "nano-banana-2"
    name: "Nano Banana 2"
    description: "Gemini 3.1 Flash Image -- reasoning-guided generation"
    endpoint: "/fal-ai/nano-banana-2"
    method: POST
    request:
      headers:
        Authorization: "Key ${FAL_KEY}"
        Content-Type: "application/json"
      body:
        prompt: "${prompt}"
        resolution: "1K"
        num_images: 1
    response:
      response_type: polling
      polling:
        status_field: "status_url"
        status_check_field: "status"
        success_value: "COMPLETED"
        failure_value: "FAILED"
        response_url_field: "response_url"
        response_envelope_field: "response"
        result_field: "images[0].url"
        interval_ms: 1000
        max_attempts: 120

image_to_3d:
  - id: "trellis-2"
    name: "Trellis 2"
    description: "High quality 3D model generation"
    endpoint: "/fal-ai/trellis-2"
    method: POST
    request:
      headers:
        Authorization: "Key ${FAL_KEY}"
        Content-Type: "application/json"
      body:
        image_url: "${image_url}"
    response:
      response_type: polling
      polling:
        status_field: "status_url"
        status_check_field: "status"
        success_value: "COMPLETED"
        failure_value: "FAILED"
        response_url_field: "response_url"
        response_envelope_field: "response"
        result_field: "model_glb.url"
        interval_ms: 2000
        max_attempts: 300
```

### Best Practices

- **Always use HTTPS** for all URLs
- **Never hardcode API keys** -- use `${ENV_VAR}` syntax
- **Verify JSONPath** expressions against actual API responses
- **Use reasonable polling intervals** to respect rate limits
- **Set adequate `max_attempts`** based on typical operation time
- **Test in mock mode first** (`make mock`) to verify config loads, then test with real API to validate response parsing
