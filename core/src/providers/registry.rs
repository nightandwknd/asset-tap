//! Provider registry for managing and discovering providers.

use super::dynamic_provider::DynamicProvider;
use super::traits::{Provider, ProviderCapability};
use include_dir::{Dir, include_dir};
use indexmap::IndexMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Embedded provider configs (all *.yaml files from providers/ directory).
static EMBEDDED_PROVIDERS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../providers");

/// Provider info collected for discovery refresh: (provider, capabilities, has_cache).
type DiscoveryTarget = (Arc<dyn Provider>, Vec<ProviderCapability>, bool);

/// Registry of available providers.
///
/// The registry maintains a collection of providers and provides
/// methods for discovering, listing, and accessing them.
#[derive(Clone)]
pub struct ProviderRegistry {
    providers: IndexMap<String, Arc<dyn Provider>>,
    /// Errors encountered while loading providers (for UI display).
    pub load_errors: Vec<ProviderLoadError>,
}

/// Error information for a provider that failed to load.
#[derive(Clone, Debug)]
pub struct ProviderLoadError {
    /// Path to the provider config file.
    pub path: String,
    /// Error message.
    pub error: String,
}

impl ProviderRegistry {
    /// Create a new provider registry with all providers from YAML/JSON configs.
    ///
    /// This automatically discovers and loads providers from:
    /// - Embedded provider configs (compiled into the binary)
    /// - User provider directory (`.dev/providers/` or OS config dir)
    ///
    /// # Examples
    ///
    /// ```
    /// use asset_tap_core::providers::ProviderRegistry;
    ///
    /// let registry = ProviderRegistry::new();
    /// println!("Loaded {} providers", registry.count());
    /// ```
    pub fn new() -> Self {
        let mut registry = Self {
            providers: IndexMap::new(),
            load_errors: Vec::new(),
        };

        // Ensure default provider configs exist
        ensure_default_providers_exist();

        // Load providers from user directory
        registry.discover_providers_from_dir(&get_user_providers_dir());

        registry
    }

    /// Start background discovery refresh for all providers with discovery enabled.
    ///
    /// This should be called after the registry is created, when a tokio runtime is available.
    /// It spawns async tasks to refresh models from provider APIs.
    pub fn start_discovery_refresh(&self) {
        self.refresh_discovery_async();
    }

    /// Refresh discovery for all providers with dynamic discovery enabled.
    ///
    /// Skips API calls for providers that already have a populated disk cache.
    /// On first launch (empty cache), fetches from provider APIs and persists results.
    /// On subsequent launches, uses cached models — no API calls.
    /// Use `force_refresh_discovery_async()` to bypass the cache (e.g., settings modal).
    fn refresh_discovery_async(&self) {
        self.run_discovery_async(false);
    }

    /// Force refresh discovery, ignoring any existing cache.
    ///
    /// This is intended for use from the settings modal when the user
    /// explicitly requests a model refresh.
    pub fn force_refresh_discovery_async(&self) {
        self.run_discovery_async(true);
    }

    /// Internal: run discovery refresh, optionally forcing a refresh even if cached.
    fn run_discovery_async(&self, force: bool) {
        let Some(providers_to_refresh) = self.collect_providers_for_discovery(force) else {
            return;
        };

        tracing::info!(
            "Starting discovery refresh for {} provider(s){}",
            providers_to_refresh.len(),
            if force { " (forced)" } else { "" }
        );

        tokio::spawn(async move {
            Self::run_discovery_for_providers(providers_to_refresh, force).await;
            tracing::info!("Discovery refresh complete");
        });
    }

    /// Refresh discovery synchronously (blocking).
    ///
    /// This is intended for CLI usage where we want to wait for discovery to complete
    /// before showing results. Skips providers that already have cached models.
    /// Pass `force: true` to bypass the cache (e.g., --refresh flag).
    pub fn refresh_discovery_blocking(&self, force: bool) -> anyhow::Result<()> {
        let Some(providers_to_refresh) = self.collect_providers_for_discovery(force) else {
            return Ok(());
        };

        tracing::info!(
            "Refreshing discovery for {} provider(s) (blocking){}...",
            providers_to_refresh.len(),
            if force { " (forced)" } else { "" }
        );

        // Use tokio::task::block_in_place to run async code synchronously
        // This works whether we're in a runtime or not
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                Self::run_discovery_for_providers(providers_to_refresh, force).await;
            })
        });

        tracing::info!("Discovery refresh complete");
        Ok(())
    }

    /// Collect providers that have discovery enabled and should be refreshed.
    ///
    /// Returns `None` when there is nothing to do (no discovery-enabled providers, or all
    /// providers already have a cached result and `force` is false). The caller should
    /// return/exit early in that case.
    fn collect_providers_for_discovery(&self, force: bool) -> Option<Vec<DiscoveryTarget>> {
        let providers_to_refresh: Vec<_> = self
            .providers
            .values()
            .filter_map(|p| {
                if let Some(dynamic) = p.as_any().downcast_ref::<DynamicProvider>() {
                    if dynamic.has_discovery() {
                        let has_cache = dynamic.has_cached_models();
                        Some((
                            Arc::clone(p),
                            dynamic.metadata().capabilities.clone(),
                            has_cache,
                        ))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        if providers_to_refresh.is_empty() {
            tracing::debug!("No providers with discovery enabled");
            return None;
        }

        // Skip the API calls when every provider already has a populated cache
        if !force
            && providers_to_refresh
                .iter()
                .all(|(_, _, has_cache)| *has_cache)
        {
            tracing::info!(
                "All {} provider(s) have cached models — skipping discovery API calls",
                providers_to_refresh.len()
            );
            return None;
        }

        Some(providers_to_refresh)
    }

    /// Async inner loop: iterate over providers and refresh each capability.
    ///
    /// Providers that already have cached models are skipped unless `force` is true.
    async fn run_discovery_for_providers(providers: Vec<DiscoveryTarget>, force: bool) {
        for (provider, capabilities, has_cache) in providers {
            if !force && has_cache {
                tracing::debug!(
                    "Skipping discovery for {} — using cached models",
                    provider.id()
                );
                continue;
            }

            if let Some(dynamic) = provider.as_any().downcast_ref::<DynamicProvider>() {
                for capability in capabilities {
                    match dynamic.refresh_models(capability).await {
                        Ok(_) => {
                            tracing::debug!(
                                "Successfully refreshed {:?} models for {}",
                                capability,
                                provider.id()
                            );
                        }
                        Err(e) => {
                            // Log but don't fail - provider will use static models
                            tracing::warn!(
                                "Failed to refresh {:?} models for {}: {}",
                                capability,
                                provider.id(),
                                e
                            );
                        }
                    }
                }
            }
        }
    }

    /// Discover and register providers from a specific directory.
    fn discover_providers_from_dir(&mut self, providers_dir: &PathBuf) {
        if !providers_dir.exists() {
            tracing::debug!("Providers directory does not exist: {:?}", providers_dir);
            return;
        }

        tracing::info!("Discovering custom providers from: {:?}", providers_dir);

        let entries = match std::fs::read_dir(providers_dir) {
            Ok(entries) => entries,
            Err(e) => {
                tracing::warn!("Failed to read providers directory: {}", e);
                return;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();

            // Skip directories
            if !path.is_file() {
                continue;
            }

            // Check file extension
            let ext = path.extension().and_then(|s| s.to_str());
            if !matches!(ext, Some("yaml") | Some("yml") | Some("json")) {
                continue;
            }

            tracing::debug!("Loading provider config: {:?}", path);

            match self.load_provider_config(&path) {
                Ok(provider) => {
                    tracing::info!(
                        "Loaded custom provider: {} ({})",
                        provider.name(),
                        provider.id()
                    );
                    self.register(Arc::new(provider));
                }
                Err(e) => {
                    let error_msg = format!("{}", e);
                    tracing::warn!("Failed to load provider from {:?}: {}", path, error_msg);
                    self.load_errors.push(ProviderLoadError {
                        path: path.display().to_string(),
                        error: error_msg,
                    });
                }
            }
        }
    }

    /// Load a provider from a config file.
    fn load_provider_config(&self, path: &std::path::Path) -> anyhow::Result<DynamicProvider> {
        #[allow(unused_mut)]
        let mut provider = if path.extension().and_then(|s| s.to_str()) == Some("json") {
            DynamicProvider::from_json_file(path)?
        } else {
            DynamicProvider::from_yaml_file(path)?
        };

        // Apply mock mode if enabled
        #[cfg(feature = "mock")]
        {
            tracing::debug!("Mock feature IS enabled in registry");
            use crate::api::is_mock_mode;
            let mock_enabled = is_mock_mode();
            tracing::debug!("Checking mock mode: {}", mock_enabled);
            if mock_enabled {
                tracing::debug!("Applying mock mode to provider");
                provider = Self::apply_mock_mode(provider)?;
            }
        }

        #[cfg(not(feature = "mock"))]
        {
            tracing::debug!("Mock feature is NOT enabled in registry");
        }

        // Set up persistent discovery cache file
        if provider.has_discovery() {
            provider.set_cache_file(get_discovery_cache_path());
        }

        // We deliberately don't log "not configured" here. Registration runs
        // before callers populate env vars from Settings::sync_to_env, so any
        // is_configured() check at this point would be wrong for the common
        // case (user saved an API key in the GUI, then ran the CLI). Callers
        // should call ProviderRegistry::log_unconfigured_providers() AFTER
        // running sync_to_env if they want a startup warning about providers
        // that still aren't configured.

        Ok(provider)
    }

    /// Apply mock mode configuration to a provider.
    #[cfg(feature = "mock")]
    fn apply_mock_mode(provider: DynamicProvider) -> anyhow::Result<DynamicProvider> {
        use crate::api::{
            is_mock_delay_enabled, is_mock_fail_enabled,
            mock::{MockApiServer, MockServerConfig, SimulatedFailure},
        };
        use std::sync::OnceLock;
        use std::sync::{Arc, Mutex};

        tracing::debug!("apply_mock_mode called for provider: {}", provider.id());

        // Use a static OnceLock to store the mock server URL
        static MOCK_INIT: OnceLock<Arc<Mutex<Option<String>>>> = OnceLock::new();

        let mock_cell = MOCK_INIT.get_or_init(|| Arc::new(Mutex::new(None)));
        let mut mock_guard = mock_cell.lock().unwrap();

        if mock_guard.is_none() {
            tracing::debug!("First provider in mock mode - starting mock server");
            // First time - need to start the mock server
            let mut config = if is_mock_delay_enabled() {
                MockServerConfig::dev_mode()
            } else {
                MockServerConfig::instant()
            };

            if is_mock_fail_enabled() {
                config.simulate_failure = Some(SimulatedFailure::Processing {
                    after_polls: 2,
                    message: "Internal server error: GPU out of memory".to_string(),
                });
            }

            // Check if we're already in a tokio runtime
            let url = if tokio::runtime::Handle::try_current().is_ok() {
                // We're in an async context - spawn a future
                std::thread::spawn(|| {
                    let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
                    let server = rt.block_on(MockApiServer::start(config));
                    let url = server.url();
                    // Keep server alive
                    Box::leak(Box::new(server));
                    Box::leak(Box::new(rt));
                    url
                })
                .join()
                .expect("Mock server thread panicked")
            } else {
                // Not in async context - can block directly
                let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
                let server = rt.block_on(MockApiServer::start(config));
                let url = server.url();
                // Keep server and runtime alive
                Box::leak(Box::new(server));
                Box::leak(Box::new(rt));
                url
            };

            tracing::info!("🎭 Mock mode enabled at {}", url);

            // Set dummy API keys for all provider env vars if not present
            for env_var in &provider.metadata().required_env_vars {
                if std::env::var(env_var).is_err() {
                    // SAFETY: Mock mode runs single-threaded before any provider
                    // threads are spawned. No concurrent env reads are possible.
                    unsafe { std::env::set_var(env_var, "mock-api-key") };
                }
            }

            *mock_guard = Some(url);
        }

        // Get the URL from the cell
        let mock_url = mock_guard.as_ref().unwrap().clone();
        drop(mock_guard); // Release lock

        // Override the provider's base_url
        provider.set_base_url(mock_url);

        // Collapse polling intervals so mock-mode pipeline runs don't pay the
        // YAML-declared 1-2 second per-stage cadence. Combined with the
        // poll-then-sleep ordering in http_client.rs, the first poll lands
        // immediately and the mock server returns COMPLETED, so a typical
        // run finishes in single-digit milliseconds per stage.
        provider.clamp_polling_interval(1);

        // Disable discovery in mock mode - use static models only
        // This prevents discovered models from having incomplete configs
        let mut provider = provider;
        provider.disable_discovery();

        Ok(provider)
    }

    /// Create a new empty provider registry.
    pub fn empty() -> Self {
        Self {
            providers: IndexMap::new(),
            load_errors: Vec::new(),
        }
    }

    /// Register a provider.
    pub fn register(&mut self, provider: Arc<dyn Provider>) {
        self.providers.insert(provider.id().to_string(), provider);
    }

    /// Get a provider by ID.
    ///
    /// # Examples
    ///
    /// ```
    /// use asset_tap_core::providers::ProviderRegistry;
    ///
    /// let registry = ProviderRegistry::new();
    /// // Get a provider by its ID (from provider config)
    /// if let Some(provider) = registry.get("my-provider") {
    ///     println!("Found provider: {}", provider.name());
    /// }
    /// ```
    pub fn get(&self, id: &str) -> Option<Arc<dyn Provider>> {
        self.providers.get(id).cloned()
    }

    /// Get the first available provider that supports the given capability.
    ///
    /// # Examples
    ///
    /// ```
    /// use asset_tap_core::providers::{ProviderRegistry, ProviderCapability};
    ///
    /// let registry = ProviderRegistry::new();
    /// if let Some(provider) = registry.get_by_capability(ProviderCapability::TextToImage) {
    ///     println!("Found text-to-image provider: {}", provider.name());
    /// }
    /// ```
    pub fn get_by_capability(&self, capability: ProviderCapability) -> Option<Arc<dyn Provider>> {
        self.providers
            .values()
            .find(|p| p.supports(capability) && p.is_available())
            .cloned()
    }

    /// Find the first available provider that exposes the given model ID for
    /// the given capability.
    ///
    /// Used by the pipeline to route a user-specified model to the right
    /// provider without requiring an explicit `image_provider` / `model_3d_provider`
    /// in `PipelineConfig`. Preserves insertion order (YAML discovery order),
    /// so multiple providers exposing the same ID resolve deterministically to
    /// the first registered one.
    pub fn find_provider_for_model(
        &self,
        capability: ProviderCapability,
        model_id: &str,
    ) -> Option<Arc<dyn Provider>> {
        self.providers
            .values()
            .find(|p| {
                p.supports(capability)
                    && p.is_available()
                    && p.list_models(capability).iter().any(|m| m.id == model_id)
            })
            .cloned()
    }

    /// List all registered provider IDs.
    pub fn list_provider_ids(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }

    /// List all registered providers (regardless of configuration).
    pub fn list_all(&self) -> Vec<Arc<dyn Provider>> {
        self.providers.values().cloned().collect()
    }

    /// List all available (configured) providers.
    ///
    /// Providers are considered "available" if they have their required environment
    /// variables set (e.g., API keys).
    ///
    /// # Examples
    ///
    /// ```
    /// use asset_tap_core::providers::ProviderRegistry;
    ///
    /// let registry = ProviderRegistry::new();
    /// for provider in registry.list_available() {
    ///     println!("Available: {} ({})", provider.name(), provider.id());
    /// }
    /// ```
    pub fn list_available(&self) -> Vec<Arc<dyn Provider>> {
        self.providers
            .values()
            .filter(|p| p.is_available())
            .cloned()
            .collect()
    }

    /// Emit a startup warning for every registered provider that isn't
    /// currently available (i.e., its required env vars aren't set).
    ///
    /// **Call this AFTER** [`crate::Settings::sync_to_env`] has populated env
    /// vars from settings.json. Calling it earlier — e.g., during registry
    /// construction — gives a false-positive warning for providers whose
    /// keys live in `settings.json` rather than the shell environment, which
    /// is the dominant case for users who configured the app via the GUI.
    ///
    /// Returns the number of providers that were unconfigured, so callers
    /// who want to suppress the warning under specific conditions (like CLI
    /// `--list-providers` mode, where the unconfigured state is the point)
    /// can decide whether to call this at all.
    pub fn log_unconfigured_providers(&self) -> usize {
        let mut count = 0;
        for provider in self.providers.values() {
            if !provider.is_available() {
                let meta = provider.metadata();
                tracing::warn!(
                    "Provider {} is not configured (missing env vars: {:?}). \
                     Set them in the GUI Settings dialog or via shell env / .env file.",
                    provider.id(),
                    meta.required_env_vars
                );
                count += 1;
            }
        }
        count
    }

    /// List all providers that support a given capability.
    pub fn list_by_capability(&self, capability: ProviderCapability) -> Vec<Arc<dyn Provider>> {
        self.providers
            .values()
            .filter(|p| p.supports(capability))
            .cloned()
            .collect()
    }

    /// Check if a provider is registered.
    pub fn has_provider(&self, id: &str) -> bool {
        self.providers.contains_key(id)
    }

    /// Get the number of registered providers.
    pub fn count(&self) -> usize {
        self.providers.len()
    }

    /// Get the number of available (configured) providers.
    pub fn count_available(&self) -> usize {
        self.providers.values().filter(|p| p.is_available()).count()
    }

    /// Check if there are any provider loading errors.
    pub fn has_load_errors(&self) -> bool {
        !self.load_errors.is_empty()
    }

    /// Get all provider loading errors.
    pub fn get_load_errors(&self) -> &[ProviderLoadError] {
        &self.load_errors
    }

    /// Get the default provider (first available provider).
    ///
    /// Returns the first provider that has all required API keys configured.
    /// Providers are checked in the order they were registered.
    pub fn get_default(&self) -> Option<Arc<dyn Provider>> {
        // Return first available provider (has API keys configured)
        self.providers.values().find(|p| p.is_available()).cloned()
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Get the user providers directory (for all provider configs).
fn get_user_providers_dir() -> PathBuf {
    use crate::settings::{config_dir, is_dev_mode};

    if is_dev_mode() {
        // In dev mode, providers stored in .dev/providers/
        PathBuf::from(crate::constants::files::dev_dirs::PROVIDERS)
    } else {
        // In release, use OS config directory
        config_dir().join("providers")
    }
}

/// Get the path for the discovery cache file.
fn get_discovery_cache_path() -> PathBuf {
    use crate::settings::{config_dir, is_dev_mode};

    if is_dev_mode() {
        PathBuf::from(".dev/discovery_cache.json")
    } else {
        config_dir().join("discovery_cache.json")
    }
}

/// Ensure default provider configs exist in user directory.
///
/// On first run, copies all embedded configs to user directory so they can be edited/removed.
/// This dynamically discovers all provider configs from the embedded providers/ directory.
fn ensure_default_providers_exist() {
    let providers_dir = get_user_providers_dir();

    // Create providers directory if it doesn't exist
    if let Err(e) = std::fs::create_dir_all(&providers_dir) {
        tracing::warn!("Failed to create providers directory: {}", e);
        return;
    }

    // Iterate through all embedded provider files
    for file in EMBEDDED_PROVIDERS.files() {
        // Only process .yaml and .yml files
        let path = file.path();
        let ext = path.extension().and_then(|s| s.to_str());
        if !matches!(ext, Some("yaml") | Some("yml")) {
            continue;
        }

        // Skip if file is in archive/ subdirectory
        if path.components().any(|c| c.as_os_str() == "archive") {
            tracing::debug!("Skipping archived provider: {:?}", path);
            continue;
        }

        // Get filename and target path
        let filename = match path.file_name() {
            Some(name) => name,
            None => continue,
        };
        let target_path = providers_dir.join(filename);

        // Get file contents
        let contents = match file.contents_utf8() {
            Some(c) => c,
            None => {
                tracing::warn!("Provider file {:?} is not valid UTF-8", path);
                continue;
            }
        };

        // Content-compare write: creates new, overwrites (with .bak) when bytes differ, or skips.
        if let Err(e) = crate::config_sync::write_with_backup(&target_path, contents, "provider") {
            tracing::warn!("Failed to write provider {:?}: {}", filename, e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::config::{
        HttpMethod, ModelConfig, ProviderConfig, ProviderMetadataConfig, RequestTemplate,
        ResponseTemplate, ResponseType,
    };
    use std::collections::HashMap;

    fn create_test_provider(id: &str) -> DynamicProvider {
        let config = ProviderConfig {
            provider: ProviderMetadataConfig {
                upload: None,
                id: id.to_string(),
                name: format!("Test {}", id),
                description: "Test provider".to_string(),
                env_vars: vec![],
                base_url: Some("https://example.com".to_string()),
                auth_format: None,
                api_key_url: None,
                website_url: None,
                docs_url: None,
                discovery: None,
            },
            text_to_image: vec![ModelConfig {
                id: "test-model".to_string(),
                name: "Test Model".to_string(),
                description: "Test".to_string(),
                endpoint: "/test".to_string(),
                method: HttpMethod::POST,
                request: RequestTemplate {
                    headers: HashMap::new(),
                    body: None,
                    multipart: None,
                },
                response: ResponseTemplate {
                    response_type: ResponseType::Binary,
                    field: None,
                    polling: None,
                },
                is_default: false,
                parameters: vec![],
            }],
            image_to_3d: vec![],
        };
        DynamicProvider::new(config)
    }

    #[test]
    fn test_empty_registry() {
        let registry = ProviderRegistry::empty();
        assert_eq!(registry.count(), 0);
        assert_eq!(registry.count_available(), 0);
        assert!(registry.get_default().is_none());
    }

    #[test]
    fn test_register_provider() {
        let mut registry = ProviderRegistry::empty();
        let provider = Arc::new(create_test_provider("test-provider"));

        registry.register(provider);
        assert_eq!(registry.count(), 1);
        assert!(registry.has_provider("test-provider"));
    }

    #[test]
    fn test_get_provider() {
        let mut registry = ProviderRegistry::empty();
        let provider = Arc::new(create_test_provider("test-provider"));
        registry.register(provider);

        let retrieved = registry.get("test-provider").unwrap();
        assert_eq!(retrieved.id(), "test-provider");

        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_get_by_capability() {
        let mut registry = ProviderRegistry::empty();

        let provider1 = Arc::new(create_test_provider("provider1"));
        let provider2 = Arc::new(create_test_provider("provider2"));

        registry.register(provider1);
        registry.register(provider2);

        // Both providers are available (no env vars required)
        let provider = registry.get_by_capability(ProviderCapability::TextToImage);
        assert!(provider.is_some());
    }

    #[test]
    fn test_list_by_capability() {
        let mut registry = ProviderRegistry::empty();

        let provider1 = Arc::new(create_test_provider("provider1"));
        let provider2 = Arc::new(create_test_provider("provider2"));

        registry.register(provider1);
        registry.register(provider2);

        let providers = registry.list_by_capability(ProviderCapability::TextToImage);
        assert_eq!(providers.len(), 2);

        let providers = registry.list_by_capability(ProviderCapability::ImageTo3D);
        assert_eq!(providers.len(), 0);
    }

    #[test]
    fn test_list_provider_ids() {
        let mut registry = ProviderRegistry::empty();

        let provider = Arc::new(create_test_provider("test-provider"));
        registry.register(provider);

        let ids = registry.list_provider_ids();
        assert_eq!(ids.len(), 1);
        assert!(ids.contains(&"test-provider".to_string()));
    }

    fn create_provider_with_text_model(provider_id: &str, model_id: &str) -> DynamicProvider {
        let config = ProviderConfig {
            provider: ProviderMetadataConfig {
                upload: None,
                id: provider_id.to_string(),
                name: format!("Test {}", provider_id),
                description: "Test".to_string(),
                env_vars: vec![],
                base_url: Some("https://example.com".to_string()),
                auth_format: None,
                api_key_url: None,
                website_url: None,
                docs_url: None,
                discovery: None,
            },
            text_to_image: vec![ModelConfig {
                id: model_id.to_string(),
                name: model_id.to_string(),
                description: "Test".to_string(),
                endpoint: "/test".to_string(),
                method: HttpMethod::POST,
                request: RequestTemplate {
                    headers: HashMap::new(),
                    body: None,
                    multipart: None,
                },
                response: ResponseTemplate {
                    response_type: ResponseType::Binary,
                    field: None,
                    polling: None,
                },
                is_default: false,
                parameters: vec![],
            }],
            image_to_3d: vec![],
        };
        DynamicProvider::new(config)
    }

    #[test]
    fn test_find_provider_for_model() {
        let mut registry = ProviderRegistry::empty();
        registry.register(Arc::new(create_provider_with_text_model(
            "fal.ai",
            "fal-ai/nano-banana",
        )));
        registry.register(Arc::new(create_provider_with_text_model(
            "meshy",
            "meshy/nano-banana",
        )));

        // Each model routes to its owning provider
        let p = registry
            .find_provider_for_model(ProviderCapability::TextToImage, "meshy/nano-banana")
            .expect("meshy model should route to meshy");
        assert_eq!(p.id(), "meshy");

        let p = registry
            .find_provider_for_model(ProviderCapability::TextToImage, "fal-ai/nano-banana")
            .expect("fal model should route to fal");
        assert_eq!(p.id(), "fal.ai");

        // Unknown model returns None — callers surface this as InvalidModel
        assert!(
            registry
                .find_provider_for_model(ProviderCapability::TextToImage, "meshy/v7/ghost")
                .is_none()
        );

        // Wrong capability (looking for a text-to-image model under ImageTo3D) returns None
        assert!(
            registry
                .find_provider_for_model(ProviderCapability::ImageTo3D, "meshy/nano-banana")
                .is_none()
        );
    }
}
