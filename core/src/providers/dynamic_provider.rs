//! Dynamic provider implementation using configuration files.

use super::config::ProviderConfig;
use super::discovery::ModelDiscoveryClient;
use super::discovery_cache::DiscoveryCache;
use super::http_client::{HttpError, HttpProviderClient};
use super::traits::{
    ImageGenerationResult, Model3DGenerationResult, ModelInfo, Provider, ProviderCapability,
    ProviderMetadata,
};
use crate::constants::http::mime;
use crate::types::Result;
use async_trait::async_trait;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::UnboundedSender;

use crate::types::Progress;

/// Convert an anyhow error from http_client into a structured Error.
///
/// If the anyhow wraps an [`HttpError`], creates a full [`crate::types::ApiError`] with
/// structured fields (URL, status code, method). Otherwise falls back to `Error::Api(String)`.
fn convert_http_error(e: anyhow::Error, provider_name: &str) -> crate::types::Error {
    match e.downcast::<HttpError>() {
        Ok(http_err) => {
            let provider = crate::types::ApiProvider::new(provider_name);
            let mut api_err = if http_err.is_queue_failure {
                crate::types::ApiError::from_model_error(provider, &http_err.body)
            } else if let Some(status) = http_err.status_code {
                crate::types::ApiError::from_response(
                    provider,
                    status,
                    &http_err.body,
                    Some(&http_err.url),
                )
            } else {
                crate::types::ApiError::from_model_error(provider, &http_err.body)
            };
            api_err.endpoint = Some(http_err.url);
            api_err.method = Some(http_err.method);
            crate::types::Error::ApiError(Box::new(api_err))
        }
        Err(e) => crate::types::Error::Api(format!("{}", e)),
    }
}

/// A provider implementation driven by external configuration.
pub struct DynamicProvider {
    /// Configuration (wrapped in Arc<Mutex<>> for interior mutability during discovery).
    config: Arc<Mutex<ProviderConfig>>,
    /// HTTP client (wrapped in Arc<Mutex<>> for recreation after config updates).
    client: Arc<Mutex<HttpProviderClient>>,
    /// Provider metadata (immutable).
    metadata: ProviderMetadata,

    /// Optional discovery client for dynamic model loading.
    /// Wrapped in Arc<Mutex<>> for recreation after config updates (e.g., mock mode).
    discovery_client: Option<Arc<Mutex<ModelDiscoveryClient>>>,

    /// Cache for discovered models.
    pub discovery_cache: Arc<Mutex<DiscoveryCache>>,

    /// Original static models from YAML config, preserved across discovery refreshes.
    /// Discovery overwrites `config.text_to_image`/`config.image_to_3d` so the HTTP client
    /// can find discovered models. These fields keep the originals for merging.
    static_text_to_image: Vec<super::config::ModelConfig>,
    static_image_to_3d: Vec<super::config::ModelConfig>,
}

/// Merge static and discovered models, with static models taking priority.
///
/// Static models appear first and are always included. Discovered models are
/// appended only if their ID is not already present in the static list. This
/// ensures static configs (which carry complete auth headers and response
/// templates) are never shadowed by incomplete discovered variants.
fn merge_models(
    static_models: &[super::config::ModelConfig],
    discovered_models: &[super::config::ModelConfig],
) -> Vec<super::config::ModelConfig> {
    let mut seen_ids: std::collections::HashSet<String> =
        static_models.iter().map(|m| m.id.clone()).collect();
    let mut merged: Vec<_> = static_models.to_vec();
    for m in discovered_models {
        if seen_ids.insert(m.id.clone()) {
            merged.push(m.clone());
        }
    }
    merged
}

impl DynamicProvider {
    /// Create a new dynamic provider from configuration.
    pub fn new(config: ProviderConfig) -> Self {
        let client = HttpProviderClient::new(config.clone());

        // Create discovery client if enabled
        let discovery_client = config
            .provider
            .discovery
            .as_ref()
            .filter(|d| d.enabled)
            .map(|_| Arc::new(Mutex::new(ModelDiscoveryClient::new(config.clone()))));

        let config_arc = Arc::new(Mutex::new(config.clone()));

        // Create metadata with capabilities based on static models + discovery config
        let mut capabilities = Vec::new();

        // Text-to-image capability
        if !config.text_to_image.is_empty()
            || config
                .provider
                .discovery
                .as_ref()
                .and_then(|d| d.text_to_image.as_ref())
                .is_some()
        {
            capabilities.push(ProviderCapability::TextToImage);
        }

        // Image-to-3D capability
        if !config.image_to_3d.is_empty()
            || config
                .provider
                .discovery
                .as_ref()
                .and_then(|d| d.image_to_3d.as_ref())
                .is_some()
        {
            capabilities.push(ProviderCapability::ImageTo3D);
        }

        let metadata = ProviderMetadata {
            id: config.provider.id.clone(),
            name: config.provider.name.clone(),
            description: config.provider.description.clone(),
            required_env_vars: config.provider.env_vars.clone(),
            capabilities,
            api_key_url: config.provider.api_key_url.clone(),
            website_url: config.provider.website_url.clone(),
            docs_url: config.provider.docs_url.clone(),
        };

        // Preserve original static models before config is moved into Arc<Mutex<>>
        let static_text_to_image = config.text_to_image.clone();
        let static_image_to_3d = config.image_to_3d.clone();

        Self {
            config: config_arc,
            client: Arc::new(Mutex::new(client)),
            metadata,
            discovery_client,
            discovery_cache: Arc::new(Mutex::new(DiscoveryCache::new())),
            static_text_to_image,
            static_image_to_3d,
        }
    }

    /// Load a dynamic provider from a YAML file.
    pub fn from_yaml_file(path: &Path) -> Result<Self> {
        let config = ProviderConfig::from_yaml_file(path).map_err(|e| {
            crate::types::Error::Config(format!("Failed to load YAML config: {}", e))
        })?;
        Ok(Self::new(config))
    }

    /// Load a dynamic provider from a JSON file.
    pub fn from_json_file(path: &Path) -> Result<Self> {
        let config = ProviderConfig::from_json_file(path).map_err(|e| {
            crate::types::Error::Config(format!("Failed to load JSON config: {}", e))
        })?;
        Ok(Self::new(config))
    }

    /// Check if this provider is configured (has required env vars set).
    pub fn is_configured(&self) -> bool {
        self.config.lock().unwrap().is_configured()
    }

    /// Check if this provider has discovery enabled.
    pub fn has_discovery(&self) -> bool {
        self.discovery_client.is_some()
    }

    /// Set the file path for persistent discovery cache.
    ///
    /// This enables the cache to survive app restarts. Call this before
    /// `start_discovery_refresh()` so the cache is loaded from disk
    /// and discovery can be skipped if cached models exist.
    ///
    /// After loading the cache from disk, syncs cached models into the
    /// provider config so that `image_to_3d()` / `text_to_image()` can
    /// find discovered models by ID.
    pub fn set_cache_file(&self, cache_file: std::path::PathBuf) {
        let mut cache = self.discovery_cache.lock().unwrap();
        *cache = DiscoveryCache::with_file(cache_file);

        // Sync cached models → config so the HTTP client can find them
        if cache.has_models() {
            self.sync_cache_to_config(&cache);
        }
    }

    /// Sync discovery cache entries into config so the HTTP client can find
    /// discovered models by ID. Mirrors what `refresh_models()` does after
    /// a successful API discovery.
    fn sync_cache_to_config(&self, cache: &DiscoveryCache) {
        let mut config = self.config.lock().unwrap();
        let mut synced = 0;

        for (provider_id, capability, cached_models) in cache.iter_entries() {
            if provider_id != self.metadata.id {
                continue;
            }

            let static_models = match capability {
                ProviderCapability::TextToImage => &self.static_text_to_image,
                ProviderCapability::ImageTo3D => &self.static_image_to_3d,
            };

            // Deduplicate: static models take priority (complete auth headers + response configs)
            let merged = merge_models(static_models, cached_models);

            synced += merged.len() - static_models.len();

            match capability {
                ProviderCapability::TextToImage => {
                    config.text_to_image = merged;
                }
                ProviderCapability::ImageTo3D => {
                    config.image_to_3d = merged;
                }
            }
        }

        if synced > 0 {
            // Recreate the HTTP client with updated config
            *self.client.lock().unwrap() = HttpProviderClient::new(config.clone());

            tracing::info!(
                "Synced {} discovered models from disk cache into config for '{}'",
                synced,
                self.metadata.id,
            );
        }
    }

    /// Check if the discovery cache already has models (from disk or memory).
    pub fn has_cached_models(&self) -> bool {
        self.discovery_cache.lock().unwrap().has_models()
    }

    /// Set a shared cancel flag that the HTTP client will check during polling.
    /// When the flag is set to `true`, any active polling loop will abort and
    /// send a server-side cancel request.
    pub fn set_cancel_flag(&self, flag: std::sync::Arc<std::sync::atomic::AtomicBool>) {
        self.client.lock().unwrap().set_cancel_flag(flag);
    }

    /// Disable model discovery for this provider.
    ///
    /// Used in mock mode to prevent discovered models with incomplete configs.
    /// After calling this, only static models from YAML will be available.
    pub fn disable_discovery(&mut self) {
        if self.discovery_client.is_some() {
            tracing::debug!("Disabling discovery for provider '{}'", self.metadata.id);
            self.discovery_client = None;
        }
    }

    /// Clamp every model's polling interval to at most `max_ms`.
    ///
    /// Used by mock mode to make pipeline tests run at memory speed instead
    /// of paying the YAML-declared 1–2 second poll cadence per stage. The
    /// real polling interval is preserved when running against a real API.
    /// `max_attempts` is left alone — clamping the interval is enough to
    /// shrink test latency without changing the loop's worst-case bound.
    pub fn clamp_polling_interval(&self, max_ms: u64) {
        let mut config = self.config.lock().unwrap();
        let mut clamped = 0usize;
        // Destructure first so the borrow checker sees two distinct mutable
        // borrows of independent fields rather than a single borrow of `config`.
        let ProviderConfig {
            text_to_image,
            image_to_3d,
            ..
        } = &mut *config;
        for model in text_to_image.iter_mut().chain(image_to_3d.iter_mut()) {
            if let Some(ref mut polling) = model.response.polling
                && polling.interval_ms > max_ms
            {
                polling.interval_ms = max_ms;
                clamped += 1;
            }
        }
        if clamped > 0 {
            tracing::debug!(
                "Clamped polling interval to {}ms for {} model(s) on provider '{}'",
                max_ms,
                clamped,
                self.metadata.id
            );
        }
    }

    /// Override the provider's base URL (used for mock mode).
    ///
    /// This also overrides upload endpoints to use relative paths instead of
    /// absolute URLs, ensuring all requests go to the mock server.
    pub fn set_base_url(&self, base_url: String) {
        tracing::debug!("Setting provider base_url to: {}", base_url);
        let mut config = self.config.lock().unwrap();
        config.provider.base_url = Some(base_url.clone());

        // Helper function to extract path from absolute URL
        let extract_path = |url: &str| -> String {
            if let Some(path_start) = url.find("://") {
                // Find the first '/' after '://'
                let after_protocol = &url[path_start + 3..];
                if let Some(slash_pos) = after_protocol.find('/') {
                    after_protocol[slash_pos..].to_string()
                } else {
                    // No path, use root
                    "/".to_string()
                }
            } else {
                url.to_string()
            }
        };

        // Also override upload endpoint if it's an absolute URL
        if let Some(ref mut upload) = config.provider.upload
            && (upload.endpoint.starts_with("http://") || upload.endpoint.starts_with("https://"))
        {
            let path = extract_path(&upload.endpoint);
            tracing::debug!(
                "Overriding upload endpoint from {} to {}",
                upload.endpoint,
                path
            );
            upload.endpoint = path;
        }

        // Also override discovery endpoints if they're absolute URLs
        if let Some(ref mut discovery) = config.provider.discovery {
            if let Some(ref mut tti) = discovery.text_to_image
                && (tti.endpoint.starts_with("http://") || tti.endpoint.starts_with("https://"))
            {
                let path = extract_path(&tti.endpoint);
                tracing::debug!(
                    "Overriding text_to_image discovery endpoint from {} to {}",
                    tti.endpoint,
                    path
                );
                tti.endpoint = path;
            }
            if let Some(ref mut i3d) = discovery.image_to_3d
                && (i3d.endpoint.starts_with("http://") || i3d.endpoint.starts_with("https://"))
            {
                let path = extract_path(&i3d.endpoint);
                tracing::debug!(
                    "Overriding image_to_3d discovery endpoint from {} to {}",
                    i3d.endpoint,
                    path
                );
                i3d.endpoint = path;
            }
        }

        // Recreate the HTTP client with the new config
        *self.client.lock().unwrap() = HttpProviderClient::new(config.clone());

        // Recreate the discovery client with the new config (if it exists)
        if let Some(ref client_arc) = self.discovery_client {
            *client_arc.lock().unwrap() = ModelDiscoveryClient::new(config.clone());
        }

        tracing::debug!(
            "Provider base_url after set: {:?}",
            config.provider.base_url
        );
        tracing::debug!(
            "Upload endpoint after set: {:?}",
            config.provider.upload.as_ref().map(|u| &u.endpoint)
        );
        tracing::debug!(
            "Discovery endpoints after set: text_to_image={:?}, image_to_3d={:?}",
            config
                .provider
                .discovery
                .as_ref()
                .and_then(|d| d.text_to_image.as_ref().map(|t| &t.endpoint)),
            config
                .provider
                .discovery
                .as_ref()
                .and_then(|d| d.image_to_3d.as_ref().map(|i| &i.endpoint))
        );
    }

    /// Refresh models from discovery API and cache them.
    ///
    /// This method fetches available models from the provider's discovery endpoint
    /// and caches them for subsequent use.
    pub async fn refresh_models(&self, capability: ProviderCapability) -> Result<()> {
        if let Some(client_arc) = &self.discovery_client {
            tracing::info!(
                "Refreshing models for {} ({:?})",
                self.metadata.id,
                capability
            );

            // Clone the client to avoid holding the lock across await
            let client = {
                let guard = client_arc.lock().unwrap();
                ModelDiscoveryClient::new(guard.config.clone())
            };

            match client.discover_models(capability).await {
                Ok(models) => {
                    let ttl = {
                        let config = self.config.lock().unwrap();
                        config
                            .provider
                            .discovery
                            .as_ref()
                            .map(|d| d.cache_ttl_secs)
                            .unwrap_or(3600)
                    };

                    let provider_id = self.metadata.id.clone();

                    // Cache the models
                    {
                        let mut cache = self.discovery_cache.lock().unwrap();
                        cache.insert(provider_id.clone(), capability, models.clone(), ttl);
                    }

                    // Also update the config so http_client can see them.
                    // Merge discovered + static so the HTTP client can find any model by ID.
                    {
                        let mut config = self.config.lock().unwrap();
                        let static_models = match capability {
                            ProviderCapability::TextToImage => &self.static_text_to_image,
                            ProviderCapability::ImageTo3D => &self.static_image_to_3d,
                        };

                        // Deduplicate: static models take priority (they have complete
                        // auth headers and correct response configs), then append discovered
                        let merged = merge_models(static_models, &models);

                        match capability {
                            ProviderCapability::TextToImage => {
                                config.text_to_image = merged;
                            }
                            ProviderCapability::ImageTo3D => {
                                config.image_to_3d = merged;
                            }
                        }

                        // Recreate the http client with updated config
                        *self.client.lock().unwrap() = HttpProviderClient::new(config.clone());

                        tracing::info!(
                            "Cached and updated {} models for {} ({:?})",
                            config.text_to_image.len() + config.image_to_3d.len(),
                            self.metadata.id,
                            capability
                        );
                    }

                    Ok(())
                }
                Err(e) => {
                    // Check if it's a timeout error
                    let error_msg = if e.to_string().contains("timeout") {
                        "Discovery request timed out. The API may be slow or unavailable."
                            .to_string()
                    } else {
                        format!("Discovery failed: {}", e)
                    };

                    // Use warn instead of error since we have cached/static fallbacks
                    tracing::warn!(
                        "Failed to discover models for {} ({:?}): {}. Will use cached models if available, otherwise static fallbacks.",
                        self.metadata.id,
                        capability,
                        error_msg
                    );
                    Err(crate::types::Error::Api(error_msg))
                }
            }
        } else {
            Ok(())
        }
    }

    /// Get models for a capability (from cache or static config).
    ///
    /// Returns merged list: discovered models from cache + static fallback models from YAML.
    /// Static models ensure important models are always available even if not in top N discovery results.
    fn get_models(&self, capability: ProviderCapability) -> Vec<super::config::ModelConfig> {
        // Use the preserved original static models (not config, which gets overwritten by discovery)
        let static_models = match capability {
            ProviderCapability::TextToImage => &self.static_text_to_image,
            ProviderCapability::ImageTo3D => &self.static_image_to_3d,
        };

        // If discovery is enabled, merge cached models with static models
        if self.discovery_client.is_some() {
            let cache = self.discovery_cache.lock().unwrap();
            if let Some(cached) = cache.get(&self.metadata.id, capability) {
                tracing::debug!(
                    "Merging {} discovered models + {} static models for {} ({:?})",
                    cached.len(),
                    static_models.len(),
                    self.metadata.id,
                    capability
                );

                // Merge: static models first (they have complete configs), then discovered
                return merge_models(static_models, cached);
            }
            tracing::debug!(
                "No cached models for {} ({:?}), using static only",
                self.metadata.id,
                capability
            );
        }

        // No discovery or cache miss: use static models only
        tracing::debug!(
            "Using {} static models for {} ({:?})",
            static_models.len(),
            self.metadata.id,
            capability
        );

        static_models.clone()
    }
}

#[async_trait]
impl Provider for DynamicProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn is_available(&self) -> bool {
        self.is_configured()
    }

    fn list_models(&self, capability: ProviderCapability) -> Vec<ModelInfo> {
        let models = self.get_models(capability);

        tracing::trace!(
            "list_models for {} ({:?}): returning {} models",
            self.metadata.id,
            capability,
            models.len()
        );

        models
            .iter()
            .map(|m| ModelInfo {
                id: m.id.clone(),
                name: m.name.clone(),
                description: Some(m.description.clone()),
                is_default: m.is_default,
                endpoint: m.endpoint.clone(),
                metadata: None,
                cost_per_run: m.cost_per_run,
                parameters: m.parameters.clone(),
            })
            .collect()
    }

    async fn text_to_image(
        &self,
        prompt: &str,
        model_id: &str,
        params: Option<&std::collections::HashMap<String, serde_json::Value>>,
        progress_tx: Option<UnboundedSender<Progress>>,
    ) -> Result<ImageGenerationResult> {
        tracing::debug!(
            "DynamicProvider::text_to_image called with model_id: {}",
            model_id
        );

        let progress = progress_tx.ok_or_else(|| {
            crate::types::Error::Pipeline("Progress channel required".to_string())
        })?;

        tracing::debug!("Calling client.generate_image");

        // Clone client to avoid holding lock across await
        let client = self.client.lock().unwrap().clone();

        let data = client
            .generate_image(prompt, model_id, params, progress)
            .await
            .map_err(|e| convert_http_error(e, &self.metadata.name))?;

        tracing::debug!("Image generation successful");

        Ok(ImageGenerationResult {
            data,
            width: None,
            height: None,
            content_type: Some(mime::IMAGE_PNG.to_string()),
        })
    }

    async fn image_to_3d(
        &self,
        image_data: &[u8],
        model_id: &str,
        params: Option<&std::collections::HashMap<String, serde_json::Value>>,
        progress_tx: Option<UnboundedSender<Progress>>,
    ) -> Result<Model3DGenerationResult> {
        let model = {
            let config = self.config.lock().unwrap();
            config
                .image_to_3d
                .iter()
                .find(|m| m.id == model_id)
                .cloned()
                .ok_or_else(|| crate::types::Error::Api(format!("Model not found: {}", model_id)))?
        };

        // Check if model needs image_url parameter (URL-based API)
        let needs_url = model
            .request
            .body
            .as_ref()
            .map(|body| body.to_string().contains("${image_url}"))
            .unwrap_or(false);

        let data = if needs_url {
            // Clone client to avoid holding lock across await
            let client = self.client.lock().unwrap().clone();

            // Provider expects a URL. Prefer the provider's upload endpoint when
            // available (fal.ai's preferred path per their docs); fall back to an
            // inline `data:image/png;base64,...` URI for providers without an
            // upload endpoint (Meshy, which only accepts URLs or data URIs).
            let has_upload = self.config.lock().unwrap().provider.upload.is_some();

            let image_url = if has_upload {
                client.upload_image(image_data).await.map_err(|e| {
                    tracing::error!(model = %model_id, "Image upload error: {:#}", e);
                    convert_http_error(e, &self.metadata.name)
                })?
            } else {
                use crate::constants::http::{MAX_DATA_URI_IMAGE_BYTES, data_uri};
                if image_data.len() > MAX_DATA_URI_IMAGE_BYTES {
                    return Err(crate::types::Error::Pipeline(format!(
                        "Image is {} bytes; provider '{}' has no upload endpoint and data-URI fallback is capped at {} bytes",
                        image_data.len(),
                        self.metadata.name,
                        MAX_DATA_URI_IMAGE_BYTES
                    )));
                }
                use base64::Engine;
                let encoded = base64::engine::general_purpose::STANDARD.encode(image_data);
                format!("{}{}", data_uri::IMAGE_PNG_BASE64, encoded)
            };

            // Execute model with image_url parameter
            client
                .execute_model_with_url(
                    &model,
                    &image_url,
                    params,
                    progress_tx.ok_or_else(|| {
                        crate::types::Error::Pipeline("Progress channel required".to_string())
                    })?,
                )
                .await
                .map_err(|e| convert_http_error(e, &self.metadata.name))?
        } else {
            // Clone client to avoid holding lock across await
            let client = self.client.lock().unwrap().clone();

            // Provider accepts file upload directly — use secure temp file
            let mut temp_file = tempfile::Builder::new()
                .prefix("asset-tap-image-")
                .suffix(".png")
                .tempfile()
                .map_err(|e| {
                    crate::types::Error::Pipeline(format!("Failed to create temp file: {}", e))
                })?;
            std::io::Write::write_all(&mut temp_file, image_data).map_err(|e| {
                crate::types::Error::Pipeline(format!("Failed to write temp file: {}", e))
            })?;

            // temp_file is automatically deleted on drop

            client
                .generate_3d(
                    temp_file.path(),
                    model_id,
                    params,
                    progress_tx.ok_or_else(|| {
                        crate::types::Error::Pipeline("Progress channel required".to_string())
                    })?,
                )
                .await
                .map_err(|e| convert_http_error(e, &self.metadata.name))?
        };

        Ok(Model3DGenerationResult {
            data,
            format: Some("glb".to_string()),
            content_type: Some(mime::MODEL_GLTF_BINARY.to_string()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::config::{
        HttpMethod, ModelConfig, ProviderMetadataConfig, RequestTemplate, ResponseTemplate,
        ResponseType,
    };
    use std::collections::HashMap;

    fn create_test_config() -> ProviderConfig {
        ProviderConfig {
            provider: ProviderMetadataConfig {
                upload: None,
                id: "test-provider".to_string(),
                name: "Test Provider".to_string(),
                description: "A test provider".to_string(),
                env_vars: vec!["TEST_API_KEY".to_string()],
                base_url: Some("https://api.example.com".to_string()),
                api_key_url: None,
                website_url: None,
                docs_url: None,
                discovery: None,
                auth_format: None,
            },
            text_to_image: vec![ModelConfig {
                id: "test-model".to_string(),
                name: "Test Model".to_string(),
                description: "A test model".to_string(),
                endpoint: "/generate".to_string(),
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
            }],
            image_to_3d: vec![],
        }
    }

    #[test]
    fn test_convert_http_error_with_status_code() {
        let http_err = HttpError {
            url: "https://api.example.com/v1/generate".to_string(),
            method: "POST".to_string(),
            status_code: Some(422),
            body: "Validation error".to_string(),
            is_queue_failure: false,
        };
        let anyhow_err: anyhow::Error = http_err.into();
        let result = convert_http_error(anyhow_err, "Test Provider");

        match result {
            crate::types::Error::ApiError(api_err) => {
                assert_eq!(
                    api_err.endpoint.as_deref(),
                    Some("https://api.example.com/v1/generate")
                );
                assert_eq!(api_err.method.as_deref(), Some("POST"));
                assert_eq!(api_err.status_code, Some(422));
                assert_eq!(api_err.provider.0, "Test Provider");
            }
            other => panic!("Expected Error::ApiError, got {:?}", other),
        }
    }

    #[test]
    fn test_convert_http_error_queue_failure() {
        let http_err = HttpError {
            url: "https://queue.fal.run/model/requests/abc/status".to_string(),
            method: "GET".to_string(),
            status_code: None,
            body: "GPU out of memory".to_string(),
            is_queue_failure: true,
        };
        let anyhow_err: anyhow::Error = http_err.into();
        let result = convert_http_error(anyhow_err, "fal.ai");

        match result {
            crate::types::Error::ApiError(api_err) => {
                assert_eq!(
                    api_err.endpoint.as_deref(),
                    Some("https://queue.fal.run/model/requests/abc/status")
                );
                assert_eq!(api_err.method.as_deref(), Some("GET"));
                assert_eq!(api_err.status_code, None);
                assert!(api_err.raw_message.contains("GPU out of memory"));
            }
            other => panic!("Expected Error::ApiError, got {:?}", other),
        }
    }

    #[test]
    fn test_convert_http_error_non_http_fallback() {
        // When the anyhow error does NOT contain an HttpError, falls back to Error::Api(String)
        let anyhow_err = anyhow::anyhow!("some random error");
        let result = convert_http_error(anyhow_err, "Test Provider");

        match result {
            crate::types::Error::Api(msg) => {
                assert_eq!(msg, "some random error");
            }
            other => panic!("Expected Error::Api, got {:?}", other),
        }
    }

    #[test]
    fn test_provider_metadata() {
        let config = create_test_config();
        let provider = DynamicProvider::new(config);

        assert_eq!(provider.id(), "test-provider");
        assert_eq!(provider.name(), "Test Provider");

        let metadata = provider.metadata();
        assert_eq!(metadata.id, "test-provider");
        assert_eq!(metadata.required_env_vars, vec!["TEST_API_KEY"]);
        assert!(
            metadata
                .capabilities
                .contains(&ProviderCapability::TextToImage)
        );
    }

    #[test]
    fn test_list_models() {
        let config = create_test_config();
        let provider = DynamicProvider::new(config);

        let models = provider.list_models(ProviderCapability::TextToImage);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "test-model");
        assert!(!models[0].is_default); // No default models with dynamic discovery

        let models_3d = provider.list_models(ProviderCapability::ImageTo3D);
        assert_eq!(models_3d.len(), 0);
    }

    #[test]
    fn test_get_model() {
        let config = create_test_config();
        let provider = DynamicProvider::new(config);

        let model = provider.get_model("test-model");
        assert!(model.is_ok());
        assert_eq!(model.unwrap().id, "test-model");

        let missing = provider.get_model("missing");
        assert!(missing.is_err());
    }

    #[test]
    fn test_default_model() {
        let config = create_test_config();
        let provider = DynamicProvider::new(config);

        // get_default_model should still work (returns first available model)
        let default = provider.get_default_model(ProviderCapability::TextToImage);
        assert!(default.is_ok());
        assert_eq!(default.unwrap().id, "test-model");
    }

    /// Build a minimal image-to-3D provider config with no upload endpoint
    /// and a polling model whose request body contains `${image_url}`.
    /// The polling response inlines the result URL so the test completes
    /// in a single status check.
    #[cfg(feature = "mock")]
    fn create_image_to_3d_config_no_upload(
        base_url: String,
        _result_url: String,
    ) -> ProviderConfig {
        use crate::providers::config::PollingConfig;
        use serde_json::json;

        ProviderConfig {
            provider: ProviderMetadataConfig {
                upload: None, // No upload endpoint → pipeline falls back to data URI
                id: "test-meshy-like".to_string(),
                name: "Test Meshy-like".to_string(),
                description: "Test".to_string(),
                env_vars: vec![],
                base_url: Some(base_url),
                api_key_url: None,
                website_url: None,
                docs_url: None,
                discovery: None,
                auth_format: None,
            },
            text_to_image: vec![],
            image_to_3d: vec![ModelConfig {
                id: "test-i23d".to_string(),
                name: "Test I23D".to_string(),
                description: "Test".to_string(),
                endpoint: "/create".to_string(),
                method: HttpMethod::POST,
                request: RequestTemplate {
                    headers: HashMap::new(),
                    body: Some(json!({ "image_url": "${image_url}" })),
                    multipart: None,
                },
                response: ResponseTemplate {
                    response_type: ResponseType::Polling,
                    field: None,
                    polling: Some(PollingConfig {
                        status_field: "result".to_string(),
                        status_url_template: Some("/status/${result}".to_string()),
                        status_check_field: "status".to_string(),
                        success_value: "SUCCEEDED".to_string(),
                        failure_value: Some("FAILED".to_string()),
                        response_url_field: None,
                        response_envelope_field: None,
                        poll_query_params: None,
                        cancel_url_template: None,
                        cancel_method: None,
                        result_field: "model_url".to_string(),
                        interval_ms: 10,
                        max_attempts: 5,
                    }),
                },
                is_default: true,
                cost_per_run: None,
                parameters: vec![],
            }],
        }
    }

    /// When a provider has no upload endpoint, image-to-3D must inline the
    /// image as a `data:image/png;base64,...` URI in the request body.
    /// Regression test for the Meshy provider path.
    #[cfg(feature = "mock")]
    #[tokio::test]
    async fn test_image_to_3d_uses_data_uri_when_no_upload_endpoint() {
        use crate::constants::http::data_uri::IMAGE_PNG_BASE64;
        use crate::constants::http::env as mock_env;
        use serde_json::json;
        use wiremock::matchers::{body_string_contains, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate as WmResponseTemplate};

        // The mock wiremock server binds to 127.0.0.1; bypass SSRF validation.
        // Tests run single-threaded (see .config/nextest.toml), so the env var
        // mutation is safe. Removed at end of test.
        unsafe { std::env::set_var(mock_env::MOCK_API, "1") };

        let server = MockServer::start().await;

        // Mock result file the pipeline will download after polling succeeds
        Mock::given(method("GET"))
            .and(path("/result.glb"))
            .respond_with(WmResponseTemplate::new(200).set_body_bytes(b"GLB_BYTES".to_vec()))
            .mount(&server)
            .await;

        // Status endpoint immediately returns SUCCEEDED with result URL
        let result_url = format!("{}/result.glb", server.uri());
        Mock::given(method("GET"))
            .and(path("/status/task-abc"))
            .respond_with(WmResponseTemplate::new(200).set_body_json(json!({
                "status": "SUCCEEDED",
                "model_url": result_url.clone(),
            })))
            .mount(&server)
            .await;

        // Create endpoint: verify body contains the data-URI prefix, then
        // return a task id so polling proceeds.
        Mock::given(method("POST"))
            .and(path("/create"))
            .and(body_string_contains(IMAGE_PNG_BASE64))
            .respond_with(WmResponseTemplate::new(200).set_body_json(json!({
                "result": "task-abc"
            })))
            .mount(&server)
            .await;

        let config = create_image_to_3d_config_no_upload(server.uri(), result_url);
        let provider = DynamicProvider::new(config);

        // Use a (progress_tx, _rx) so the pipeline can emit progress.
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let result = provider
            .image_to_3d(b"fake-png-bytes", "test-i23d", None, Some(tx))
            .await;

        assert!(result.is_ok(), "expected success, got {:?}", result.err());
        assert_eq!(result.unwrap().data, b"GLB_BYTES".to_vec());

        unsafe { std::env::remove_var(mock_env::MOCK_API) };
    }

    /// Oversized images must be rejected before hitting the network.
    #[cfg(feature = "mock")]
    #[tokio::test]
    async fn test_image_to_3d_rejects_oversize_image_in_data_uri_mode() {
        use crate::constants::http::MAX_DATA_URI_IMAGE_BYTES;

        // Mock server isn't contacted — the size check fails first — but we
        // still provide a base_url so config construction is realistic.
        let config = create_image_to_3d_config_no_upload(
            "http://127.0.0.1:1".to_string(),
            "http://127.0.0.1:1/unused".to_string(),
        );
        let provider = DynamicProvider::new(config);

        let oversize = vec![0u8; MAX_DATA_URI_IMAGE_BYTES + 1];
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let result = provider
            .image_to_3d(&oversize, "test-i23d", None, Some(tx))
            .await;

        let err = result.expect_err("oversize image should be rejected");
        match err {
            crate::types::Error::Pipeline(msg) => {
                assert!(
                    msg.contains("data-URI fallback is capped"),
                    "unexpected message: {}",
                    msg
                );
            }
            other => panic!("expected Error::Pipeline, got {:?}", other),
        }
    }
}
