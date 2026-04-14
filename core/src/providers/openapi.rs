//! OpenAPI 3.0 schema parser for generating model configurations.
//!
//! Parses OpenAPI schemas from provider discovery APIs and generates
//! executable `ModelConfig` structures with request/response templates.
//!
//! Key capabilities:
//! - Resolves `$ref` references to `components/schemas/*`
//! - Extracts output schema from the GET result endpoint to determine `result_field`
//! - Maps input fields to template variables (`${prompt}`, `${image_url}`)
//! - Handles varying field names across models (e.g., `input_image_url` vs `image_url`)

use super::config::{
    HttpMethod, ModelConfig, PollingConfig, RequestTemplate, ResponseTemplate, ResponseType,
};
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::HashMap;

/// OpenAPI 3.0 schema parser.
pub struct OpenApiParser;

impl OpenApiParser {
    /// Parse an OpenAPI 3.0 schema and generate a ModelConfig.
    pub fn parse_model(
        id: String,
        name: String,
        description: Option<String>,
        openapi: &Value,
        _base_url: &str,
    ) -> Result<ModelConfig> {
        let paths = openapi
            .get("paths")
            .context("OpenAPI schema missing 'paths' field")?;

        // Find the main generation endpoint (POST)
        let (endpoint, operation) = Self::find_post_endpoint(paths)?;

        // Parse request template (with $ref resolution)
        let request = Self::parse_request_template(operation, openapi)?;

        // Parse response template by finding the output schema from the GET result endpoint
        let response = Self::parse_response_template(paths, openapi)?;

        Ok(ModelConfig {
            id,
            name,
            description: description.unwrap_or_default(),
            endpoint: endpoint.to_string(),
            method: HttpMethod::POST,
            request,
            response,
            is_default: false,
            cost_per_run: None,
            parameters: vec![],
        })
    }

    /// Resolve a `$ref` like `#/components/schemas/Foo` to the actual schema value.
    fn resolve_ref<'a>(openapi: &'a Value, ref_path: &str) -> Option<&'a Value> {
        let path = ref_path.strip_prefix("#/")?;
        let mut current = openapi;
        for segment in path.split('/') {
            current = current.get(segment)?;
        }
        Some(current)
    }

    /// Resolve a schema that may be a `$ref`, `allOf`, or an inline schema.
    ///
    /// Handles three patterns:
    /// - Direct `$ref`: `{"$ref": "#/components/schemas/Foo"}`
    /// - `allOf` wrapper: `{"allOf": [{"$ref": "#/components/schemas/Foo"}]}`
    /// - Inline schema: returned as-is
    fn resolve_schema<'a>(openapi: &'a Value, schema: &'a Value) -> &'a Value {
        if let Some(ref_path) = schema.get("$ref").and_then(|r| r.as_str()) {
            return Self::resolve_ref(openapi, ref_path).unwrap_or(schema);
        }
        // Handle allOf with a single $ref (common fal.ai pattern for File references)
        if let Some(all_of) = schema.get("allOf").and_then(|a| a.as_array()) {
            for item in all_of {
                if let Some(ref_path) = item.get("$ref").and_then(|r| r.as_str())
                    && let Some(resolved) = Self::resolve_ref(openapi, ref_path)
                {
                    return resolved;
                }
            }
        }
        schema
    }

    /// Find the main POST endpoint in the OpenAPI paths.
    fn find_post_endpoint(paths: &Value) -> Result<(&str, &Value)> {
        let paths_obj = paths
            .as_object()
            .context("OpenAPI 'paths' is not an object")?;

        for (path, methods) in paths_obj.iter() {
            if path.contains("/status") || path.contains("/cancel") || path.contains("/requests/") {
                continue;
            }

            if let Some(post_op) = methods.get("post") {
                return Ok((path, post_op));
            }
        }

        Err(anyhow::anyhow!("No POST endpoint found in OpenAPI schema"))
    }

    /// Find the GET result endpoint (e.g., `/{model}/requests/{request_id}`)
    /// and extract its response schema.
    fn find_result_schema<'a>(paths: &'a Value, openapi: &'a Value) -> Option<&'a Value> {
        let paths_obj = paths.as_object()?;

        for (path, methods) in paths_obj.iter() {
            // Match: /requests/{request_id} but NOT /status or /cancel
            if path.contains("/requests/")
                && !path.contains("/status")
                && !path.contains("/cancel")
                && let Some(get_op) = methods.get("get")
            {
                // Extract response schema: responses -> 200 -> content -> application/json -> schema
                let schema = get_op
                    .get("responses")
                    .and_then(|r| r.get("200"))
                    .and_then(|r| r.get("content"))
                    .and_then(|c| c.get("application/json"))
                    .and_then(|aj| aj.get("schema"))?;

                return Some(Self::resolve_schema(openapi, schema));
            }
        }

        None
    }

    /// Determine the `result_field` path from an output schema.
    ///
    /// Analyzes the schema properties to find the downloadable file URL:
    /// - For image outputs: `images[0].url` (array of Image/File objects)
    /// - For 3D outputs: `model_glb.url` (File object with url)
    /// - Fallback: first property that is a File-like object with a `url` field
    fn extract_result_field(schema: &Value, openapi: &Value) -> Option<String> {
        let properties = schema.get("properties")?.as_object()?;

        // Priority 1: `images` array (text-to-image models)
        if let Some(images_prop) = properties.get("images") {
            let resolved = Self::resolve_schema(openapi, images_prop);
            if resolved.get("type").and_then(|t| t.as_str()) == Some("array") {
                // Verify the items have a `url` field
                if let Some(items) = resolved.get("items") {
                    let items_resolved = Self::resolve_schema(openapi, items);
                    if Self::has_url_property(items_resolved, openapi) {
                        return Some("images[0].url".to_string());
                    }
                }
            }
        }

        // Priority 2: `model_glb` object (3D models like trellis, hunyuan)
        if let Some(glb_prop) = properties.get("model_glb") {
            let resolved = Self::resolve_schema(openapi, glb_prop);
            if Self::has_url_property(resolved, openapi) {
                return Some("model_glb.url".to_string());
            }
        }

        // Priority 3: `model_urls.glb` (alternative 3D model format)
        if let Some(urls_prop) = properties.get("model_urls") {
            let resolved = Self::resolve_schema(openapi, urls_prop);
            if let Some(inner_props) = resolved.get("properties").and_then(|p| p.as_object())
                && let Some(glb_prop) = inner_props.get("glb")
            {
                let glb_resolved = Self::resolve_schema(openapi, glb_prop);
                if Self::has_url_property(glb_resolved, openapi) {
                    return Some("model_urls.glb.url".to_string());
                }
            }
        }

        // Priority 4: `image` object (single image output)
        if let Some(image_prop) = properties.get("image") {
            let resolved = Self::resolve_schema(openapi, image_prop);
            if Self::has_url_property(resolved, openapi) {
                return Some("image.url".to_string());
            }
        }

        // Priority 5: `video` object
        if let Some(video_prop) = properties.get("video") {
            let resolved = Self::resolve_schema(openapi, video_prop);
            if Self::has_url_property(resolved, openapi) {
                return Some("video.url".to_string());
            }
        }

        // Priority 6: `output` URL string
        if let Some(output_prop) = properties.get("output") {
            let resolved = Self::resolve_schema(openapi, output_prop);
            if resolved.get("type").and_then(|t| t.as_str()) == Some("string") {
                return Some("output".to_string());
            }
            if Self::has_url_property(resolved, openapi) {
                return Some("output.url".to_string());
            }
        }

        // Priority 7: Scan all properties for the first File-like object with a url
        for (key, prop) in properties {
            // Skip non-output fields
            if matches!(
                key.as_str(),
                "seed" | "timings" | "has_nsfw_concepts" | "prompt" | "debug" | "logs"
            ) {
                continue;
            }
            let resolved = Self::resolve_schema(openapi, prop);

            // Check if it's an array of File-like objects
            if resolved.get("type").and_then(|t| t.as_str()) == Some("array")
                && let Some(items) = resolved.get("items")
            {
                let items_resolved = Self::resolve_schema(openapi, items);
                if Self::has_url_property(items_resolved, openapi) {
                    return Some(format!("{}[0].url", key));
                }
            }

            // Check if it's a File-like object
            if Self::has_url_property(resolved, openapi) {
                return Some(format!("{}.url", key));
            }
        }

        None
    }

    /// Check if a schema has a `url` property (i.e., is a File-like object).
    fn has_url_property(schema: &Value, openapi: &Value) -> bool {
        let schema = Self::resolve_schema(openapi, schema);
        schema
            .get("properties")
            .and_then(|p| p.get("url"))
            .is_some()
    }

    // --- Request parsing ---

    /// Parse request template from OpenAPI operation, resolving $ref.
    fn parse_request_template(operation: &Value, openapi: &Value) -> Result<RequestTemplate> {
        let schema_ref = operation
            .get("requestBody")
            .and_then(|rb| rb.get("content"))
            .and_then(|c| c.get("application/json"))
            .and_then(|aj| aj.get("schema"))
            .context("Request body schema not found in OpenAPI")?;

        let schema = Self::resolve_schema(openapi, schema_ref);

        let body = Self::build_body_template(schema, openapi)?;

        Ok(RequestTemplate {
            headers: HashMap::new(),
            body: Some(body),
            multipart: None,
        })
    }

    /// Build a request body template from an OpenAPI schema.
    ///
    /// Maps known semantic fields to template variables and includes defaults.
    fn build_body_template(schema: &Value, openapi: &Value) -> Result<Value> {
        let schema = Self::resolve_schema(openapi, schema);
        let properties = schema
            .get("properties")
            .and_then(|p| p.as_object())
            .context("Schema missing 'properties' field")?;

        let required: Vec<&str> = schema
            .get("required")
            .and_then(|r| r.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();

        let mut body = serde_json::Map::new();

        for (key, prop) in properties {
            let resolved_prop = Self::resolve_schema(openapi, prop);

            // Map known fields to template variables
            let value = if Self::is_prompt_field(key, resolved_prop) {
                Value::String("${prompt}".to_string())
            } else if Self::is_image_url_field(key, resolved_prop) {
                Value::String("${image_url}".to_string())
            } else if let Some(default) = resolved_prop.get("default") {
                default.clone()
            } else if required.contains(&key.as_str()) {
                // Required field without a default — skip it rather than send garbage.
                // Our template variables above handle the important required fields.
                continue;
            } else {
                continue;
            };

            body.insert(key.clone(), value);
        }

        Ok(Value::Object(body))
    }

    /// Check if a field name/schema represents a text prompt input.
    fn is_prompt_field(key: &str, _prop: &Value) -> bool {
        matches!(key, "prompt" | "text" | "description" | "input_text")
    }

    /// Check if a field name/schema represents an image URL input.
    fn is_image_url_field(key: &str, _prop: &Value) -> bool {
        // Match by exact field name patterns used across fal.ai models.
        // We intentionally do NOT fuzzy-match on "contains image" because
        // fields like "image_size", "num_images" are not image URLs.
        matches!(
            key,
            "image"
                | "image_url"
                | "input_image"
                | "input_image_url"
                | "source_image"
                | "source_image_url"
                | "init_image"
                | "init_image_url"
                | "front_image_url"
                | "reference_image_url"
        )
    }

    // --- Response parsing ---

    /// Parse response template by extracting the output schema from the GET result endpoint.
    fn parse_response_template(paths: &Value, openapi: &Value) -> Result<ResponseTemplate> {
        let result_field = if let Some(output_schema) = Self::find_result_schema(paths, openapi) {
            Self::extract_result_field(output_schema, openapi)
                .unwrap_or_else(|| "images[0].url".to_string())
        } else {
            // No GET result endpoint found — fall back based on available schemas.
            // Try to find the output schema by naming convention in components/schemas.
            Self::find_output_schema_by_name(openapi)
                .and_then(|schema| Self::extract_result_field(schema, openapi))
                .unwrap_or_else(|| "images[0].url".to_string())
        };

        tracing::debug!("Extracted result_field from OpenAPI: {}", result_field);

        Ok(ResponseTemplate {
            response_type: ResponseType::Polling,
            field: None,
            polling: Some(PollingConfig {
                status_field: "status_url".to_string(),
                status_url_template: None,
                result_field,
                status_check_field: "status".to_string(),
                success_value: "COMPLETED".to_string(),
                failure_value: Some("FAILED".to_string()),
                response_url_field: Some("response_url".to_string()),
                response_envelope_field: Some("response".to_string()),
                poll_query_params: None,
                cancel_url_template: None,
                cancel_method: None,
                interval_ms: 2000,
                max_attempts: 180,
            }),
        })
    }

    /// Find the output schema by naming convention (schema name ending in "Output").
    fn find_output_schema_by_name(openapi: &Value) -> Option<&Value> {
        let schemas = openapi
            .get("components")
            .and_then(|c| c.get("schemas"))
            .and_then(|s| s.as_object())?;

        for (name, schema) in schemas {
            if name.ends_with("Output") && name != "QueueStatus" {
                return Some(schema);
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_text_to_image_schema() {
        let openapi = serde_json::json!({
            "paths": {
                "/generate": {
                    "post": {
                        "requestBody": {
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "properties": {
                                            "prompt": {
                                                "type": "string",
                                                "description": "The text prompt"
                                            },
                                            "seed": {
                                                "type": "integer",
                                                "default": 42
                                            },
                                            "num_images": {
                                                "type": "integer",
                                                "default": 1
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        let model = OpenApiParser::parse_model(
            "test-model".to_string(),
            "Test Model".to_string(),
            Some("A test model".to_string()),
            &openapi,
            "https://api.example.com",
        )
        .unwrap();

        assert_eq!(model.id, "test-model");
        assert_eq!(model.name, "Test Model");
        assert_eq!(model.description, "A test model");
        assert_eq!(model.endpoint, "/generate");
        assert_eq!(model.method, HttpMethod::POST);

        // Check request body
        let body = model.request.body.unwrap();
        assert_eq!(body.get("prompt").unwrap().as_str().unwrap(), "${prompt}");
        assert_eq!(body.get("seed").unwrap().as_i64().unwrap(), 42);
        assert_eq!(body.get("num_images").unwrap().as_i64().unwrap(), 1);

        // Check response config
        assert_eq!(model.response.response_type, ResponseType::Polling);
        assert!(model.response.polling.is_some());
    }

    #[test]
    fn test_parse_image_to_3d_schema() {
        let openapi = serde_json::json!({
            "paths": {
                "/fal-ai/trellis-2": {
                    "post": {
                        "requestBody": {
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "properties": {
                                            "image_url": {
                                                "type": "string",
                                                "description": "URL of the input image"
                                            },
                                            "resolution": {
                                                "type": "integer",
                                                "default": 1024
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        let model = OpenApiParser::parse_model(
            "trellis-2".to_string(),
            "TRELLIS 2".to_string(),
            None,
            &openapi,
            "https://queue.fal.run",
        )
        .unwrap();

        let body = model.request.body.unwrap();
        assert_eq!(
            body.get("image_url").unwrap().as_str().unwrap(),
            "${image_url}"
        );
        assert_eq!(body.get("resolution").unwrap().as_i64().unwrap(), 1024);
    }

    #[test]
    fn test_parse_skips_status_endpoints() {
        let openapi = serde_json::json!({
            "paths": {
                "/generate/requests/{request_id}/status": {
                    "get": {
                        "description": "Check status"
                    }
                },
                "/generate": {
                    "post": {
                        "requestBody": {
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "properties": {
                                            "prompt": {"type": "string"}
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        let model =
            OpenApiParser::parse_model("test".to_string(), "Test".to_string(), None, &openapi, "")
                .unwrap();

        assert_eq!(model.endpoint, "/generate");
    }

    #[test]
    fn test_parse_missing_paths() {
        let openapi = serde_json::json!({
            "openapi": "3.0.0",
            "info": {"title": "Test", "version": "1.0"}
        });

        let result =
            OpenApiParser::parse_model("test".to_string(), "Test".to_string(), None, &openapi, "");

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing 'paths'"));
    }

    #[test]
    fn test_parse_no_post_endpoint() {
        let openapi = serde_json::json!({
            "paths": {
                "/status": {
                    "get": {
                        "description": "Get status"
                    }
                }
            }
        });

        let result =
            OpenApiParser::parse_model("test".to_string(), "Test".to_string(), None, &openapi, "");

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No POST endpoint"));
    }

    /// Test with a realistic fal.ai text-to-image OpenAPI schema using $ref.
    #[test]
    fn test_parse_fal_text_to_image_with_refs() {
        let openapi = serde_json::json!({
            "paths": {
                "/fal-ai/flux/dev": {
                    "post": {
                        "requestBody": {
                            "required": true,
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "$ref": "#/components/schemas/FluxDevInput"
                                    }
                                }
                            }
                        },
                        "responses": {
                            "200": {
                                "content": {
                                    "application/json": {
                                        "schema": {
                                            "$ref": "#/components/schemas/QueueStatus"
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                "/fal-ai/flux/dev/requests/{request_id}": {
                    "get": {
                        "responses": {
                            "200": {
                                "content": {
                                    "application/json": {
                                        "schema": {
                                            "$ref": "#/components/schemas/FluxDevOutput"
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                "/fal-ai/flux/dev/requests/{request_id}/status": {
                    "get": { "description": "Check status" }
                }
            },
            "components": {
                "schemas": {
                    "FluxDevInput": {
                        "type": "object",
                        "properties": {
                            "prompt": {
                                "type": "string",
                                "description": "The text prompt"
                            },
                            "image_size": {
                                "type": "string",
                                "default": "landscape_4_3"
                            },
                            "num_images": {
                                "type": "integer",
                                "default": 1
                            },
                            "seed": {
                                "type": "integer"
                            }
                        },
                        "required": ["prompt"]
                    },
                    "FluxDevOutput": {
                        "type": "object",
                        "properties": {
                            "images": {
                                "type": "array",
                                "items": {
                                    "$ref": "#/components/schemas/Image"
                                }
                            },
                            "seed": { "type": "integer" },
                            "timings": { "type": "object" }
                        }
                    },
                    "Image": {
                        "type": "object",
                        "properties": {
                            "url": { "type": "string" },
                            "width": { "type": "integer" },
                            "height": { "type": "integer" },
                            "content_type": { "type": "string" }
                        }
                    },
                    "QueueStatus": {
                        "type": "object",
                        "properties": {
                            "status": { "type": "string" },
                            "request_id": { "type": "string" },
                            "response_url": { "type": "string" },
                            "status_url": { "type": "string" }
                        }
                    }
                }
            }
        });

        let model = OpenApiParser::parse_model(
            "fal-ai/flux/dev".to_string(),
            "FLUX.1 [dev]".to_string(),
            Some("FLUX dev model".to_string()),
            &openapi,
            "https://queue.fal.run",
        )
        .unwrap();

        // Request body should resolve the $ref and map prompt
        let body = model.request.body.unwrap();
        assert_eq!(body.get("prompt").unwrap().as_str().unwrap(), "${prompt}");
        assert_eq!(
            body.get("image_size").unwrap().as_str().unwrap(),
            "landscape_4_3"
        );
        assert_eq!(body.get("num_images").unwrap().as_i64().unwrap(), 1);
        // seed has no default and is not required, should be skipped
        assert!(body.get("seed").is_none());

        // Response should extract result_field from the GET result endpoint
        let polling = model.response.polling.unwrap();
        assert_eq!(polling.result_field, "images[0].url");
    }

    /// Test with a realistic fal.ai image-to-3D OpenAPI schema using $ref.
    #[test]
    fn test_parse_fal_image_to_3d_with_refs() {
        let openapi = serde_json::json!({
            "paths": {
                "/fal-ai/hunyuan-3d/v3.1/pro/image-to-3d": {
                    "post": {
                        "requestBody": {
                            "required": true,
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "$ref": "#/components/schemas/Hunyuan3dInput"
                                    }
                                }
                            }
                        }
                    }
                },
                "/fal-ai/hunyuan-3d/v3.1/pro/image-to-3d/requests/{request_id}": {
                    "get": {
                        "responses": {
                            "200": {
                                "content": {
                                    "application/json": {
                                        "schema": {
                                            "$ref": "#/components/schemas/Hunyuan3dOutput"
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                "/fal-ai/hunyuan-3d/v3.1/pro/image-to-3d/requests/{request_id}/status": {
                    "get": { "description": "Check status" }
                }
            },
            "components": {
                "schemas": {
                    "Hunyuan3dInput": {
                        "type": "object",
                        "properties": {
                            "input_image_url": {
                                "type": "string",
                                "description": "Front view image URL"
                            },
                            "enable_pbr": {
                                "type": "boolean",
                                "default": false
                            }
                        },
                        "required": ["input_image_url"]
                    },
                    "Hunyuan3dOutput": {
                        "type": "object",
                        "properties": {
                            "model_glb": {
                                "$ref": "#/components/schemas/File"
                            },
                            "model_urls": {
                                "$ref": "#/components/schemas/ModelUrls"
                            },
                            "thumbnail": {
                                "$ref": "#/components/schemas/File"
                            }
                        }
                    },
                    "File": {
                        "type": "object",
                        "properties": {
                            "url": { "type": "string" },
                            "content_type": { "type": "string" },
                            "file_name": { "type": "string" },
                            "file_size": { "type": "integer" }
                        }
                    },
                    "ModelUrls": {
                        "type": "object",
                        "properties": {
                            "glb": { "$ref": "#/components/schemas/File" },
                            "obj": { "$ref": "#/components/schemas/File" },
                            "fbx": { "$ref": "#/components/schemas/File" }
                        }
                    }
                }
            }
        });

        let model = OpenApiParser::parse_model(
            "fal-ai/hunyuan-3d".to_string(),
            "Hunyuan 3D".to_string(),
            None,
            &openapi,
            "https://queue.fal.run",
        )
        .unwrap();

        // Request body should map input_image_url to ${image_url}
        let body = model.request.body.unwrap();
        assert_eq!(
            body.get("input_image_url").unwrap().as_str().unwrap(),
            "${image_url}"
        );
        assert!(!body.get("enable_pbr").unwrap().as_bool().unwrap());

        // Response should extract model_glb.url from the GET result endpoint
        let polling = model.response.polling.unwrap();
        assert_eq!(polling.result_field, "model_glb.url");
    }

    /// Test that output schema can be found by naming convention when no GET endpoint exists.
    #[test]
    fn test_parse_output_schema_by_naming_convention() {
        let openapi = serde_json::json!({
            "paths": {
                "/generate": {
                    "post": {
                        "requestBody": {
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "properties": {
                                            "prompt": { "type": "string" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "components": {
                "schemas": {
                    "GenerateOutput": {
                        "type": "object",
                        "properties": {
                            "images": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "url": { "type": "string" },
                                        "width": { "type": "integer" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        let model =
            OpenApiParser::parse_model("test".to_string(), "Test".to_string(), None, &openapi, "")
                .unwrap();

        let polling = model.response.polling.unwrap();
        assert_eq!(polling.result_field, "images[0].url");
    }

    /// Test model_urls.glb.url fallback when model_glb is not present.
    #[test]
    fn test_parse_model_urls_glb_fallback() {
        let openapi = serde_json::json!({
            "paths": {
                "/generate-3d": {
                    "post": {
                        "requestBody": {
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "properties": {
                                            "image_url": { "type": "string" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                "/generate-3d/requests/{request_id}": {
                    "get": {
                        "responses": {
                            "200": {
                                "content": {
                                    "application/json": {
                                        "schema": {
                                            "type": "object",
                                            "properties": {
                                                "model_urls": {
                                                    "type": "object",
                                                    "properties": {
                                                        "glb": {
                                                            "type": "object",
                                                            "properties": {
                                                                "url": { "type": "string" }
                                                            }
                                                        },
                                                        "obj": {
                                                            "type": "object",
                                                            "properties": {
                                                                "url": { "type": "string" }
                                                            }
                                                        }
                                                    }
                                                },
                                                "seed": { "type": "integer" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        let model = OpenApiParser::parse_model(
            "test-3d".to_string(),
            "Test 3D".to_string(),
            None,
            &openapi,
            "",
        )
        .unwrap();

        let polling = model.response.polling.unwrap();
        assert_eq!(polling.result_field, "model_urls.glb.url");
    }

    /// Test that allOf $ref pattern (used by real fal.ai API) is resolved correctly.
    /// The actual API returns: `"model_glb": {"allOf": [{"$ref": "#/components/schemas/File"}]}`
    /// instead of a direct `$ref`.
    #[test]
    fn test_parse_allof_ref_pattern() {
        let openapi = serde_json::json!({
            "paths": {
                "/fal-ai/hunyuan-3d/v3.1/pro/image-to-3d": {
                    "post": {
                        "requestBody": {
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "$ref": "#/components/schemas/Input"
                                    }
                                }
                            }
                        }
                    }
                },
                "/fal-ai/hunyuan-3d/v3.1/pro/image-to-3d/requests/{request_id}": {
                    "get": {
                        "responses": {
                            "200": {
                                "content": {
                                    "application/json": {
                                        "schema": {
                                            "$ref": "#/components/schemas/Output"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "components": {
                "schemas": {
                    "Input": {
                        "type": "object",
                        "properties": {
                            "input_image_url": { "type": "string" }
                        },
                        "required": ["input_image_url"]
                    },
                    "Output": {
                        "type": "object",
                        "properties": {
                            "model_glb": {
                                "title": "Model Glb",
                                "allOf": [{ "$ref": "#/components/schemas/File" }]
                            },
                            "thumbnail": {
                                "allOf": [{ "$ref": "#/components/schemas/File" }]
                            }
                        }
                    },
                    "File": {
                        "type": "object",
                        "properties": {
                            "url": { "type": "string" },
                            "content_type": { "type": "string" },
                            "file_name": { "type": "string" },
                            "file_size": { "type": "integer" }
                        }
                    }
                }
            }
        });

        let model = OpenApiParser::parse_model(
            "test-allof".to_string(),
            "Test AllOf".to_string(),
            None,
            &openapi,
            "",
        )
        .unwrap();

        let polling = model.response.polling.unwrap();
        assert_eq!(polling.result_field, "model_glb.url");
    }
}
