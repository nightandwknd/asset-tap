//! Provider configuration schema and parsing.
//!
//! Allows defining providers via YAML/JSON configuration files.

use crate::constants::polling;
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Complete provider configuration loaded from YAML/JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// Provider metadata (ID, name, description, etc.).
    pub provider: ProviderMetadataConfig,

    /// Text-to-image models offered by this provider.
    #[serde(default)]
    pub text_to_image: Vec<ModelConfig>,

    /// Image-to-3D models offered by this provider.
    #[serde(default)]
    pub image_to_3d: Vec<ModelConfig>,
}

/// Provider metadata from config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderMetadataConfig {
    /// Unique provider ID (e.g., "stability-ai").
    pub id: String,

    /// Human-readable provider name (e.g., "Stability AI").
    pub name: String,

    /// Provider description.
    pub description: String,

    /// Environment variables required for authentication.
    pub env_vars: Vec<String>,

    /// Optional base URL for API endpoints.
    #[serde(default)]
    pub base_url: Option<String>,

    /// Optional file upload configuration for providers that need public URLs.
    #[serde(default)]
    pub upload: Option<UploadConfig>,

    /// Format string for the Authorization header (e.g., "Key ${API_KEY}", "Bearer ${API_KEY}").
    /// Supports `${VAR}` interpolation with env_vars. Defaults to "Key ${API_KEY}" if not specified.
    #[serde(default)]
    pub auth_format: Option<String>,

    /// URL where users can get API keys (e.g., <https://platform.openai.com/api-keys>).
    #[serde(default)]
    pub api_key_url: Option<String>,

    /// Provider website URL.
    #[serde(default)]
    pub website_url: Option<String>,

    /// Provider documentation URL.
    #[serde(default)]
    pub docs_url: Option<String>,

    /// Optional dynamic model discovery configuration.
    #[serde(default)]
    pub discovery: Option<DiscoveryConfig>,
}

/// Model configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    /// Unique model ID within this provider.
    pub id: String,

    /// Human-readable model name.
    pub name: String,

    /// Model description.
    pub description: String,

    /// API endpoint for this model (relative to base_url or absolute).
    pub endpoint: String,

    /// HTTP method (GET, POST, etc.).
    #[serde(default = "default_http_method")]
    pub method: HttpMethod,

    /// Request template.
    pub request: RequestTemplate,

    /// Response extraction template.
    pub response: ResponseTemplate,

    /// Whether this model is the default for its capability.
    #[serde(default)]
    pub is_default: bool,

    /// Estimated cost per run in USD (for cost tracking in history).
    #[serde(default)]
    pub cost_per_run: Option<f64>,

    /// User-tunable parameters for this model.
    /// Declared in YAML, exposed in GUI as sliders/checkboxes/dropdowns.
    #[serde(default)]
    pub parameters: Vec<ParameterDef>,
}

/// Definition of a user-tunable model parameter.
///
/// Declared in provider YAML under a model's `parameters` list.
/// Each parameter maps to a key in the request body that can be
/// overridden at runtime via the GUI or CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterDef {
    /// Parameter name (must match a key in the request body).
    pub name: String,

    /// Human-readable label shown in the GUI.
    pub label: String,

    /// Tooltip description explaining what this parameter does.
    #[serde(default)]
    pub description: Option<String>,

    /// Parameter type (determines which GUI widget is rendered).
    #[serde(rename = "type")]
    pub param_type: ParameterType,

    /// Default value (used when user hasn't overridden).
    pub default: serde_json::Value,

    /// Minimum value (for float/integer sliders).
    #[serde(default)]
    pub min: Option<f64>,

    /// Maximum value (for float/integer sliders).
    #[serde(default)]
    pub max: Option<f64>,

    /// Step increment (for float/integer sliders).
    #[serde(default)]
    pub step: Option<f64>,

    /// Allowed values (for select/dropdown type).
    #[serde(default)]
    pub options: Option<Vec<serde_json::Value>>,
}

/// Type of a tunable parameter (determines GUI widget).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ParameterType {
    /// Floating-point slider.
    Float,
    /// Integer slider.
    Integer,
    /// Checkbox.
    Boolean,
    /// Text input.
    String,
    /// Dropdown from predefined options.
    Select,
}

fn default_http_method() -> HttpMethod {
    HttpMethod::POST
}

/// HTTP method.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    GET,
    POST,
    PUT,
    DELETE,
    PATCH,
}

impl HttpMethod {
    pub fn as_str(&self) -> &str {
        match self {
            HttpMethod::GET => "GET",
            HttpMethod::POST => "POST",
            HttpMethod::PUT => "PUT",
            HttpMethod::DELETE => "DELETE",
            HttpMethod::PATCH => "PATCH",
        }
    }
}

/// Request template with variable interpolation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestTemplate {
    /// HTTP headers (supports ${VAR} interpolation).
    #[serde(default)]
    pub headers: HashMap<String, String>,

    /// Request body as JSON (supports ${VAR} interpolation).
    #[serde(default)]
    pub body: Option<serde_json::Value>,

    /// Optional multipart form fields for file uploads.
    #[serde(default)]
    pub multipart: Option<MultipartTemplate>,
}

/// Multipart form template for file uploads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultipartTemplate {
    /// Field name for the file upload.
    pub file_field: String,

    /// Additional form fields (supports ${VAR} interpolation).
    #[serde(default)]
    pub fields: HashMap<String, String>,
}

/// File upload configuration for providers that need to upload images to get URLs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadConfig {
    /// Upload endpoint (relative to base_url or absolute).
    pub endpoint: String,

    /// HTTP method for upload.
    #[serde(default = "default_http_method")]
    pub method: HttpMethod,

    /// Request template for upload.
    pub request: UploadRequestTemplate,

    /// Response template for extracting the file URL.
    pub response: UploadResponseTemplate,
}

/// Upload request template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadRequestTemplate {
    /// HTTP headers (supports ${VAR} interpolation).
    #[serde(default)]
    pub headers: HashMap<String, String>,

    /// Upload method (multipart or initiate-then-put for two-step uploads).
    #[serde(rename = "type")]
    pub upload_type: UploadType,

    /// Field name for the file in multipart upload.
    #[serde(default)]
    pub file_field: Option<String>,

    /// Additional form fields for multipart (supports ${VAR} interpolation).
    #[serde(default)]
    pub fields: HashMap<String, String>,

    /// For initiate-then-put: JSON body to get upload URL.
    #[serde(default)]
    pub initiate_body: Option<serde_json::Value>,
}

/// Upload type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UploadType {
    /// Single-step multipart upload.
    Multipart,
    /// Two-step: initiate to get upload URL, then PUT file.
    InitiateThenPut,
}

/// Upload response template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadResponseTemplate {
    /// Field containing the final file URL.
    pub file_url_field: String,

    /// For initiate-then-put: field containing the upload URL.
    #[serde(default)]
    pub upload_url_field: Option<String>,
}

/// Response extraction template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseTemplate {
    /// Response type (json, base64, binary, url).
    pub response_type: ResponseType,

    /// JSONPath or field name to extract result.
    #[serde(default)]
    pub field: Option<String>,

    /// Optional polling configuration for async APIs.
    #[serde(default)]
    pub polling: Option<PollingConfig>,
}

/// Response type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ResponseType {
    /// Direct JSON response with image data.
    Json,
    /// Base64-encoded image in response.
    Base64,
    /// Binary image data.
    Binary,
    /// URL to download result.
    Url,
    /// Polling-based async response.
    Polling,
}

/// Polling configuration for async APIs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollingConfig {
    /// Field containing the status URL or ID.
    /// Required unless `status_url_template` is set.
    #[serde(default)]
    pub status_field: String,

    /// Optional template for constructing the poll URL from the initial response.
    /// Supports `${field}` placeholders resolved against the initial response JSON
    /// (nested paths like `${data.id}` and array indices like `${items[0]}` are
    /// supported). Used for providers that return `{"result": "<task-id>"}`
    /// instead of a full status URL (e.g., Meshy).
    /// Relative paths are resolved against `provider.base_url`.
    /// When set, `status_field` is ignored.
    #[serde(default)]
    pub status_url_template: Option<String>,

    /// Field containing the final result URL.
    pub result_field: String,

    /// Field indicating completion status.
    pub status_check_field: String,

    /// Value indicating success.
    pub success_value: String,

    /// Optional value indicating failure.
    #[serde(default)]
    pub failure_value: Option<String>,

    /// Optional field in the status response containing the URL to fetch the actual result.
    /// When set, the result is fetched from this URL instead of extracting it from the status response.
    /// Used by queue-based APIs (e.g., fal.ai) where the status endpoint and result endpoint differ.
    #[serde(default)]
    pub response_url_field: Option<String>,

    /// Optional envelope field wrapping the actual model output in the result response.
    /// When set, the result_field path is applied to the value inside this envelope.
    /// For example, fal.ai wraps output in `{"response": { ...model output... }}`.
    #[serde(default)]
    pub response_envelope_field: Option<String>,

    /// Additional query parameters to append to polling URLs (e.g., "?logs=1").
    #[serde(default)]
    pub poll_query_params: Option<String>,

    /// Template for constructing cancel URLs from status URLs.
    /// Uses `${status_url}` as the base. Default behavior replaces `/status` with `/cancel`.
    #[serde(default)]
    pub cancel_url_template: Option<String>,

    /// HTTP method used for the cancel request. Defaults to PUT (fal.ai's convention).
    /// Providers using REST-style cancel should set this to DELETE (e.g., Meshy).
    #[serde(default)]
    pub cancel_method: Option<HttpMethod>,

    /// Poll interval in milliseconds.
    #[serde(default = "default_poll_interval")]
    pub interval_ms: u64,

    /// Maximum polling attempts.
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
}

fn default_poll_interval() -> u64 {
    polling::TEXT_TO_IMAGE_INTERVAL_MS
}

fn default_max_attempts() -> u32 {
    polling::DEFAULT_MAX_ATTEMPTS
}

impl ProviderConfig {
    /// Load provider configuration from a YAML file.
    pub fn from_yaml_file(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read provider config: {:?}", path))?;

        // Parse to Value first to handle YAML merge keys (<<: *anchor)
        let mut value: serde_yaml_ng::Value = serde_yaml_ng::from_str(&contents)
            .map_err(|e| anyhow::anyhow!("Failed to parse YAML {:?}: {}", path, e))?;

        // Recursively apply merge keys to handle nested merges
        Self::apply_merge_recursive(&mut value)?;

        // Then deserialize to our struct
        let config: ProviderConfig = serde_yaml_ng::from_value(value)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize config {:?}: {}", path, e))?;

        config.validate()?;

        Ok(config)
    }

    /// Recursively apply YAML merge keys throughout the value tree.
    fn apply_merge_recursive(value: &mut serde_yaml_ng::Value) -> Result<()> {
        // First apply merge at this level
        value
            .apply_merge()
            .map_err(|e| anyhow::anyhow!("Failed to apply YAML merge keys: {}", e))?;

        // Then recurse into nested structures
        match value {
            serde_yaml_ng::Value::Mapping(map) => {
                for (_, v) in map.iter_mut() {
                    Self::apply_merge_recursive(v)?;
                }
            }
            serde_yaml_ng::Value::Sequence(seq) => {
                for v in seq.iter_mut() {
                    Self::apply_merge_recursive(v)?;
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// Load provider configuration from a JSON file.
    pub fn from_json_file(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read provider config: {:?}", path))?;

        let config: ProviderConfig = serde_json::from_str(&contents)
            .with_context(|| format!("Failed to parse JSON config: {:?}", path))?;

        config.validate()?;

        Ok(config)
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<()> {
        // Validate provider metadata
        if self.provider.id.is_empty() {
            return Err(anyhow!("Provider ID cannot be empty"));
        }
        if self.provider.name.is_empty() {
            return Err(anyhow!("Provider name cannot be empty"));
        }
        if self.provider.env_vars.is_empty() {
            return Err(anyhow!(
                "Provider must specify at least one environment variable"
            ));
        }

        // Validate models - allow empty if discovery is enabled
        let has_static_models = !self.text_to_image.is_empty() || !self.image_to_3d.is_empty();
        let has_discovery = self
            .provider
            .discovery
            .as_ref()
            .map(|d| d.enabled)
            .unwrap_or(false);

        if !has_static_models && !has_discovery {
            return Err(anyhow!(
                "Provider must define at least one model or enable discovery"
            ));
        }

        // Validate each model
        for model in self.text_to_image.iter().chain(self.image_to_3d.iter()) {
            if model.id.is_empty() {
                return Err(anyhow!("Model ID cannot be empty"));
            }
            if model.endpoint.is_empty() {
                return Err(anyhow!("Model endpoint cannot be empty"));
            }
        }

        Ok(())
    }

    /// Check if this provider is configured (has required env vars set).
    pub fn is_configured(&self) -> bool {
        self.provider
            .env_vars
            .iter()
            .all(|var| std::env::var(var).map(|v| !v.is_empty()).unwrap_or(false))
    }

    /// Get the primary API key from environment.
    pub fn get_api_key(&self) -> Option<String> {
        self.provider
            .env_vars
            .first()
            .and_then(|var| std::env::var(var).ok())
    }

    /// Format the Authorization header value using the provider's auth_format.
    ///
    /// If `auth_format` is set (e.g., "Bearer ${API_KEY}"), interpolates env vars into it.
    /// Otherwise falls back to "Key {api_key}" (fal.ai default format).
    pub fn format_auth_header(&self) -> Option<String> {
        let api_key = self.get_api_key()?;

        if let Some(ref format_str) = self.provider.auth_format {
            // Interpolate env vars in the auth format string
            let mut result = format_str.clone();
            for var in &self.provider.env_vars {
                if let Ok(val) = std::env::var(var) {
                    result = result.replace(&format!("${{{}}}", var), &val);
                }
            }
            Some(result)
        } else {
            // Default fallback (fal.ai "Key" format)
            Some(format!("Key {}", api_key))
        }
    }
}

/// Dynamic model discovery configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryConfig {
    /// Whether discovery is enabled for this provider.
    pub enabled: bool,

    /// Discovery configuration for text-to-image models.
    #[serde(default)]
    pub text_to_image: Option<DiscoveryEndpoint>,

    /// Discovery configuration for image-to-3D models.
    #[serde(default)]
    pub image_to_3d: Option<DiscoveryEndpoint>,

    /// Cache TTL in seconds (how long to keep discovered models).
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl_secs: u64,

    /// Whether the discovery endpoint requires authentication.
    #[serde(default)]
    pub require_auth: bool,

    /// Timeout for discovery requests in seconds (prevents blocking startup).
    #[serde(default = "default_discovery_timeout")]
    pub timeout_secs: u64,
}

fn default_cache_ttl() -> u64 {
    3600 // 1 hour
}

fn default_discovery_timeout() -> u64 {
    5 // 5 seconds
}

/// Discovery endpoint configuration for a specific capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryEndpoint {
    /// API endpoint URL for model discovery.
    pub endpoint: String,

    /// Query parameters to send with the discovery request.
    #[serde(default)]
    pub params: HashMap<String, String>,

    /// JSONPath to extract the models array from the response.
    #[serde(default = "default_models_field")]
    pub models_field: String,

    /// Field mapping configuration (how to extract model info from API response).
    #[serde(default)]
    pub field_mapping: DiscoveryFieldMapping,

    /// Whether to fetch OpenAPI schemas for discovered models.
    #[serde(default = "default_fetch_schemas")]
    pub fetch_schemas: bool,

    /// Query parameter name to enable OpenAPI schema expansion.
    #[serde(default)]
    pub schema_expand_param: Option<String>,
}

fn default_models_field() -> String {
    "models".to_string()
}

fn default_fetch_schemas() -> bool {
    true
}

/// Maps API response fields to our internal model structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryFieldMapping {
    /// Field containing the model ID.
    #[serde(default = "default_id_field")]
    pub id_field: String,

    /// Field containing the model display name.
    #[serde(default = "default_name_field")]
    pub name_field: String,

    /// Optional field containing the model description.
    #[serde(default)]
    pub description_field: Option<String>,

    /// Optional field containing the model endpoint path.
    #[serde(default)]
    pub endpoint_field: Option<String>,

    /// Optional field containing the model status.
    #[serde(default)]
    pub status_field: Option<String>,

    /// Value indicating the model is active/available (for status filtering).
    #[serde(default)]
    pub active_status_value: Option<String>,

    /// Field containing the OpenAPI 3.0 schema (when expanded).
    #[serde(default)]
    pub openapi_field: Option<String>,
}

fn default_id_field() -> String {
    "endpoint_id".to_string()
}

fn default_name_field() -> String {
    "display_name".to_string()
}

impl Default for DiscoveryFieldMapping {
    fn default() -> Self {
        Self {
            id_field: default_id_field(),
            name_field: default_name_field(),
            description_field: Some("description".to_string()),
            endpoint_field: Some("endpoint_id".to_string()),
            status_field: Some("status".to_string()),
            active_status_value: Some("active".to_string()),
            openapi_field: Some("openapi".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_yaml_parsing() {
        let yaml = r#"
        provider:
            id: "test-provider"
            name: "Test Provider"
            description: "A test provider"
            env_vars:
                - TEST_API_KEY
            base_url: "https://api.example.com"

        text_to_image:
            -   id: "model-1"
                name: "Model 1"
                description: "Test model"
                endpoint: "/generate"
                method: POST
                request:
                    headers:
                        Authorization: "Bearer ${TEST_API_KEY}"
                    body:
                        prompt: "${prompt}"
                response:
                    response_type: json
                    field: "image_url"
        "#;

        let config: ProviderConfig = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(config.provider.id, "test-provider");
        assert_eq!(config.text_to_image.len(), 1);
        assert_eq!(config.text_to_image[0].id, "model-1");
    }

    #[test]
    fn test_validation() {
        let mut config = ProviderConfig {
            provider: ProviderMetadataConfig {
                id: "test".to_string(),
                name: "Test".to_string(),
                description: "Test".to_string(),
                env_vars: vec!["KEY".to_string()],
                base_url: None,
                upload: None,
                api_key_url: None,
                website_url: None,
                docs_url: None,
                discovery: None,
                auth_format: None,
            },
            text_to_image: vec![],
            image_to_3d: vec![],
        };

        // Should fail - no models
        assert!(config.validate().is_err());

        // Add upload config
        config.provider.upload = None;

        // Add a model
        config.text_to_image.push(ModelConfig {
            id: "test".to_string(),
            name: "Test".to_string(),
            description: "Test".to_string(),
            endpoint: "/test".to_string(),
            method: HttpMethod::POST,
            request: RequestTemplate {
                headers: HashMap::new(),
                body: None,
                multipart: None,
            },
            response: ResponseTemplate {
                response_type: ResponseType::Json,
                field: None,
                polling: None,
            },
            is_default: false,
            cost_per_run: None,
            parameters: vec![],
        });

        // Should pass
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_parameter_def_deserialization() {
        let yaml = r#"
        provider:
            id: "test"
            name: "Test"
            description: "Test"
            env_vars: ["KEY"]
        text_to_image:
            -   id: "model-1"
                name: "Model 1"
                description: "Test model"
                endpoint: "/generate"
                method: POST
                request:
                    body:
                        prompt: "${prompt}"
                        guidance_scale: 3.5
                response:
                    response_type: json
                    field: "url"
                parameters:
                    -   name: "guidance_scale"
                        label: "Guidance Scale"
                        description: "Controls adherence to prompt"
                        type: float
                        default: 3.5
                        min: 1.0
                        max: 20.0
                        step: 0.5
                    -   name: "steps"
                        label: "Steps"
                        type: integer
                        default: 28
                        min: 1
                        max: 50
        "#;

        let config: ProviderConfig = serde_yaml_ng::from_str(yaml).unwrap();
        let model = &config.text_to_image[0];
        assert_eq!(model.parameters.len(), 2);

        let guidance = &model.parameters[0];
        assert_eq!(guidance.name, "guidance_scale");
        assert_eq!(guidance.label, "Guidance Scale");
        assert_eq!(guidance.param_type, ParameterType::Float);
        assert_eq!(guidance.default, serde_json::json!(3.5));
        assert_eq!(guidance.min, Some(1.0));
        assert_eq!(guidance.max, Some(20.0));
        assert_eq!(guidance.step, Some(0.5));

        let steps = &model.parameters[1];
        assert_eq!(steps.param_type, ParameterType::Integer);
        assert_eq!(steps.default, serde_json::json!(28));
        assert!(steps.description.is_none());
    }

    #[test]
    fn test_parameter_def_optional_fields() {
        let yaml = r#"
        provider:
            id: "test"
            name: "Test"
            description: "Test"
            env_vars: ["KEY"]
        text_to_image:
            -   id: "model-1"
                name: "Model 1"
                description: "Test"
                endpoint: "/gen"
                request:
                    body:
                        prompt: "${prompt}"
                response:
                    response_type: json
                parameters:
                    -   name: "enable_pbr"
                        label: "PBR"
                        type: boolean
                        default: true
                    -   name: "topology"
                        label: "Topology"
                        type: select
                        default: "triangle"
                        options: ["triangle", "quad"]
        "#;

        let config: ProviderConfig = serde_yaml_ng::from_str(yaml).unwrap();
        let params = &config.text_to_image[0].parameters;

        let pbr = &params[0];
        assert_eq!(pbr.param_type, ParameterType::Boolean);
        assert_eq!(pbr.default, serde_json::json!(true));
        assert!(pbr.min.is_none());

        let topo = &params[1];
        assert_eq!(topo.param_type, ParameterType::Select);
        let opts = topo.options.as_ref().unwrap();
        assert_eq!(opts.len(), 2);
        assert_eq!(opts[0], serde_json::json!("triangle"));
    }

    #[test]
    fn test_model_without_parameters_backwards_compatible() {
        let yaml = r#"
        provider:
            id: "test"
            name: "Test"
            description: "Test"
            env_vars: ["KEY"]
        text_to_image:
            -   id: "model-1"
                name: "Model 1"
                description: "Test"
                endpoint: "/gen"
                request:
                    body:
                        prompt: "${prompt}"
                response:
                    response_type: json
        "#;

        let config: ProviderConfig = serde_yaml_ng::from_str(yaml).unwrap();
        assert!(config.text_to_image[0].parameters.is_empty());
    }
}
