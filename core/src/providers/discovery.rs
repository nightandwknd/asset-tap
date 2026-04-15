//! Model discovery client for fetching available models from provider APIs.
//!
//! Implements dynamic model discovery by querying provider APIs and parsing
//! the responses to generate executable model configurations.

use super::config::{DiscoveryConfig, DiscoveryEndpoint, ModelConfig, ProviderConfig};
use super::http_client::resolve_url;
use super::openapi::OpenApiParser;
use super::traits::ProviderCapability;
use crate::constants::http::headers;
use anyhow::{Context, Result};
use serde_json::Value;
use std::time::Duration;

/// HTTP client for discovering models from provider APIs.
pub struct ModelDiscoveryClient {
    pub(super) config: ProviderConfig,
    client: reqwest::Client,
}

impl ModelDiscoveryClient {
    /// Create a new discovery client for the given provider.
    pub fn new(config: ProviderConfig) -> Self {
        let timeout = config
            .provider
            .discovery
            .as_ref()
            .map(|d| d.timeout_secs)
            .unwrap_or(5);

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout))
            .build()
            .expect("Failed to create HTTP client");

        Self { config, client }
    }

    /// Discover models for a given capability.
    ///
    /// Returns a list of fully-configured models with generated templates.
    pub async fn discover_models(
        &self,
        capability: ProviderCapability,
    ) -> Result<Vec<ModelConfig>> {
        let discovery = self
            .config
            .provider
            .discovery
            .as_ref()
            .context("Discovery not configured for this provider")?;

        if !discovery.enabled {
            return Ok(Vec::new());
        }

        let endpoint_config = match capability {
            ProviderCapability::TextToImage => &discovery.text_to_image,
            ProviderCapability::ImageTo3D => &discovery.image_to_3d,
        };

        let endpoint_config = endpoint_config
            .as_ref()
            .context("Capability not configured for discovery")?;

        self.fetch_and_parse_models(endpoint_config, discovery)
            .await
    }

    /// Fetch models from the discovery endpoint and parse them.
    async fn fetch_and_parse_models(
        &self,
        endpoint: &DiscoveryEndpoint,
        discovery: &DiscoveryConfig,
    ) -> Result<Vec<ModelConfig>> {
        // Resolve URL (handle relative vs absolute)
        let url = resolve_url(self.config.provider.base_url.as_deref(), &endpoint.endpoint);

        // Build HTTP request
        let mut request = self.client.get(&url);

        // Add query parameters
        for (key, value) in &endpoint.params {
            request = request.query(&[(key, value)]);
        }

        // Add schema expansion parameter if enabled
        if endpoint.fetch_schemas
            && let Some(param) = &endpoint.schema_expand_param
        {
            request = request.query(&[(param.as_str(), "openapi-3.0")]);
        }

        // Add authentication if required
        if discovery.require_auth
            && let Some(auth_value) = self.config.format_auth_header()
        {
            request = request.header(headers::AUTHORIZATION, auth_value);
        }

        // Execute request
        tracing::info!(
            "Discovering models from {} for provider '{}'",
            url,
            self.config.provider.id
        );

        let response = request
            .send()
            .await
            .with_context(|| format!("Failed to fetch models from discovery endpoint: {}", url))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Discovery endpoint returned error {}: {}",
                status,
                body
            ));
        }

        let data: Value = response
            .json()
            .await
            .context("Failed to parse discovery response as JSON")?;

        // Extract models array from response
        let models_array = self.extract_models_array(&data, &endpoint.models_field)?;

        // Parse each model
        let mut models = Vec::new();
        for model_data in models_array {
            match self.parse_model(model_data, endpoint).await {
                Ok(model) => models.push(model),
                Err(e) => {
                    // Log but don't fail the entire discovery
                    tracing::warn!("Failed to parse model from discovery response: {}", e);
                }
            }
        }

        tracing::info!(
            "Discovered {} models for provider '{}' ({})",
            models.len(),
            self.config.provider.id,
            self.config.provider.name
        );

        Ok(models)
    }

    /// Extract a field from JSON data using JSONPath-like syntax (e.g., "metadata.display_name").
    fn extract_field<'a>(data: &'a Value, path: &str) -> Option<&'a Value> {
        let mut current = data;
        for segment in path.split('.') {
            current = current.get(segment)?;
        }
        Some(current)
    }

    /// Create a basic model config without OpenAPI schema.
    ///
    /// This fallback is used when OpenAPI parsing fails or isn't available.
    /// It generates a best-guess config based on heuristics. The result_field
    /// is guessed from common patterns and may not match all models.
    fn create_basic_model_config(
        id: String,
        name: String,
        description: String,
        endpoint: String,
    ) -> Result<ModelConfig> {
        use super::config::{
            HttpMethod, PollingConfig, RequestTemplate, ResponseTemplate, ResponseType,
        };
        use std::collections::HashMap;

        // Determine capability based on endpoint/id patterns
        let lower_id = id.to_lowercase();
        let lower_endpoint = endpoint.to_lowercase();
        let lower_name = name.to_lowercase();
        let is_3d = lower_endpoint.contains("3d")
            || lower_id.contains("3d")
            || lower_name.contains("3d")
            || lower_endpoint.contains("mesh")
            || lower_id.contains("trellis")
            || lower_id.contains("hunyuan");

        let result_field = if is_3d {
            "model_glb.url"
        } else {
            "images[0].url"
        };

        tracing::warn!(
            "No OpenAPI schema for model '{}' — using guessed result_field '{}'. \
            This model may fail if its output format differs.",
            id,
            result_field
        );

        // Build request body with appropriate template variable
        let mut body = serde_json::Map::new();
        if is_3d {
            body.insert(
                "image_url".to_string(),
                serde_json::Value::String("${image_url}".to_string()),
            );
        } else {
            body.insert(
                "prompt".to_string(),
                serde_json::Value::String("${prompt}".to_string()),
            );
        }

        Ok(ModelConfig {
            id,
            name,
            description,
            endpoint: format!("/{}", endpoint.trim_start_matches('/')),
            method: HttpMethod::POST,
            request: RequestTemplate {
                headers: HashMap::new(),
                body: Some(serde_json::Value::Object(body)),
                multipart: None,
            },
            response: ResponseTemplate {
                response_type: ResponseType::Polling,
                field: None,
                polling: Some(PollingConfig {
                    status_field: "status_url".to_string(),
                    status_url_template: None,
                    result_field: result_field.to_string(),
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
            },
            is_default: false,
            parameters: vec![],
        })
    }

    /// Extract the models array from the API response using JSONPath-like syntax.
    fn extract_models_array<'a>(&self, data: &'a Value, path: &str) -> Result<&'a Vec<Value>> {
        let mut current = data;
        for segment in path.split('.') {
            current = current
                .get(segment)
                .ok_or_else(|| anyhow::anyhow!("Field '{}' not found in response", segment))?;
        }

        current
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Models field '{}' is not an array", path))
    }

    /// Parse a single model from the discovery response.
    async fn parse_model(&self, data: &Value, endpoint: &DiscoveryEndpoint) -> Result<ModelConfig> {
        let mapping = &endpoint.field_mapping;

        // Extract required fields (supporting nested paths like "metadata.display_name")
        let id = Self::extract_field(data, &mapping.id_field)
            .and_then(|v| v.as_str())
            .context("Model ID field not found")?
            .to_string();

        let name = Self::extract_field(data, &mapping.name_field)
            .and_then(|v| v.as_str())
            .with_context(|| {
                format!(
                    "Model name field '{}' not found in model data",
                    mapping.name_field
                )
            })?
            .to_string();

        // Filter by status if configured
        if let (Some(status_field), Some(active_value)) =
            (&mapping.status_field, &mapping.active_status_value)
            && let Some(status) = Self::extract_field(data, status_field).and_then(|v| v.as_str())
            && status != active_value
        {
            return Err(anyhow::anyhow!(
                "Model '{}' has status '{}' (expected '{}')",
                id,
                status,
                active_value
            ));
        }

        // Extract optional description
        let description = mapping
            .description_field
            .as_ref()
            .and_then(|f| Self::extract_field(data, f))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Try to generate ModelConfig from OpenAPI schema if available
        if endpoint.fetch_schemas
            && let Some(openapi_field) = &mapping.openapi_field
            && let Some(openapi) = Self::extract_field(data, openapi_field)
        {
            match OpenApiParser::parse_model(
                id.clone(),
                name.clone(),
                description.clone(),
                openapi,
                self.config.provider.base_url.as_deref().unwrap_or_default(),
            ) {
                Ok(model) => return Ok(model),
                Err(e) => {
                    tracing::debug!(
                        "Failed to parse OpenAPI for model '{}': {}. Falling back to basic template.",
                        id,
                        e
                    );
                    // Continue to fallback below
                }
            }
        }

        // Fallback: Generate basic model config without OpenAPI
        // Use the endpoint_id as the endpoint path
        let endpoint_path = mapping
            .endpoint_field
            .as_ref()
            .and_then(|f| Self::extract_field(data, f))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| id.clone());

        // Create a basic model config that assumes standard FAL patterns
        Self::create_basic_model_config(id, name, description.unwrap_or_default(), endpoint_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::config::{DiscoveryFieldMapping, ProviderMetadataConfig};

    fn create_test_config() -> ProviderConfig {
        ProviderConfig {
            provider: ProviderMetadataConfig {
                id: "test-provider".to_string(),
                name: "Test Provider".to_string(),
                description: "Test".to_string(),
                env_vars: vec!["TEST_KEY".to_string()],
                base_url: Some("https://api.example.com".to_string()),
                upload: None,
                api_key_url: None,
                website_url: None,
                docs_url: None,
                auth_format: None,
                discovery: Some(DiscoveryConfig {
                    enabled: true,
                    text_to_image: Some(DiscoveryEndpoint {
                        endpoint: "https://api.example.com/models".to_string(),
                        params: std::collections::HashMap::new(),
                        models_field: "models".to_string(),
                        field_mapping: DiscoveryFieldMapping::default(),
                        fetch_schemas: true,
                        schema_expand_param: Some("expand".to_string()),
                    }),
                    image_to_3d: None,
                    cache_ttl_secs: 3600,
                    require_auth: false,
                    timeout_secs: 5,
                }),
            },
            text_to_image: vec![],
            image_to_3d: vec![],
        }
    }

    #[test]
    fn test_extract_models_array() {
        let data = serde_json::json!({
            "models": [
                {"id": "model-1"},
                {"id": "model-2"}
            ]
        });

        let config = create_test_config();
        let client = ModelDiscoveryClient::new(config);

        let models = client.extract_models_array(&data, "models").unwrap();
        assert_eq!(models.len(), 2);
    }

    #[test]
    fn test_extract_nested_models_array() {
        let data = serde_json::json!({
            "data": {
                "models": [
                    {"id": "model-1"}
                ]
            }
        });

        let config = create_test_config();
        let client = ModelDiscoveryClient::new(config);

        let models = client.extract_models_array(&data, "data.models").unwrap();
        assert_eq!(models.len(), 1);
    }

    #[test]
    fn test_extract_missing_field() {
        let data = serde_json::json!({
            "other": []
        });

        let config = create_test_config();
        let client = ModelDiscoveryClient::new(config);

        let result = client.extract_models_array(&data, "models");
        assert!(result.is_err());
    }
}
