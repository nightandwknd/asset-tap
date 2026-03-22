//! Discovery cache for discovered models.
//!
//! Provides both in-memory and file-backed caching of model configurations
//! fetched from provider discovery APIs. The file cache persists across app
//! restarts to avoid unnecessary API calls on every launch.

use super::config::ModelConfig;
use super::traits::ProviderCapability;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Cached models with timestamp and TTL (in-memory).
#[derive(Debug, Clone)]
pub struct CachedModels {
    /// The discovered model configurations.
    pub models: Vec<ModelConfig>,
    /// When this cache entry was created.
    pub cached_at: Instant,
    /// How long this entry is valid.
    pub ttl: Duration,
}

impl CachedModels {
    /// Create a new cache entry.
    pub fn new(models: Vec<ModelConfig>, ttl_secs: u64) -> Self {
        Self {
            models,
            cached_at: Instant::now(),
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    /// Check if this cache entry has expired.
    pub fn is_expired(&self) -> bool {
        self.cached_at.elapsed() > self.ttl
    }

    /// Get the age of this cache entry.
    pub fn age(&self) -> Duration {
        self.cached_at.elapsed()
    }
}

/// Serializable entry for file-backed cache.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileCacheEntry {
    provider_id: String,
    capability: ProviderCapability,
    models: Vec<ModelConfig>,
    /// Unix timestamp (seconds) when this entry was cached.
    cached_at_unix: u64,
}

/// Top-level file cache structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileCacheData {
    /// Cache format version for future compatibility.
    version: u32,
    entries: Vec<FileCacheEntry>,
}

/// Discovery cache with in-memory and file-backed persistence.
///
/// Stores model configurations by provider ID and capability.
/// On startup, loads from disk to avoid API calls. Saves to disk after
/// successful discovery. Manual refresh from settings invalidates and re-fetches.
pub struct DiscoveryCache {
    /// In-memory cache: (provider_id, capability) -> cached models
    cache: HashMap<(String, ProviderCapability), CachedModels>,
    /// Path to the cache file on disk.
    cache_file: Option<PathBuf>,
}

impl DiscoveryCache {
    /// Create a new empty cache (no file backing).
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            cache_file: None,
        }
    }

    /// Create a cache backed by a file. Loads existing cache from disk if present.
    pub fn with_file(cache_file: PathBuf) -> Self {
        let mut cache = Self {
            cache: HashMap::new(),
            cache_file: Some(cache_file),
        };
        cache.load_from_disk();
        cache
    }

    /// Check if there are any cached models (memory or disk).
    pub fn has_models(&self) -> bool {
        !self.cache.is_empty()
    }

    /// Get cached models if they exist and haven't expired.
    pub fn get(
        &self,
        provider_id: &str,
        capability: ProviderCapability,
    ) -> Option<&Vec<ModelConfig>> {
        let key = (provider_id.to_string(), capability);
        self.cache.get(&key).and_then(|cached| {
            if cached.is_expired() {
                None
            } else {
                Some(&cached.models)
            }
        })
    }

    /// Insert models into the cache with specified TTL, and persist to disk.
    pub fn insert(
        &mut self,
        provider_id: String,
        capability: ProviderCapability,
        models: Vec<ModelConfig>,
        ttl_secs: u64,
    ) {
        let key = (provider_id, capability);
        self.cache.insert(key, CachedModels::new(models, ttl_secs));
        self.save_to_disk();
    }

    /// Invalidate (remove) cached models for a specific provider and capability.
    pub fn invalidate(&mut self, provider_id: &str, capability: ProviderCapability) {
        let key = (provider_id.to_string(), capability);
        self.cache.remove(&key);
        self.save_to_disk();
    }

    /// Clear all cached models (memory and disk).
    pub fn clear(&mut self) {
        self.cache.clear();
        self.delete_disk_cache();
    }

    /// Get the number of cache entries.
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Check if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Iterate all cache entries as (provider_id, capability, models).
    pub fn iter_entries(
        &self,
    ) -> impl Iterator<Item = (&str, ProviderCapability, &Vec<ModelConfig>)> {
        self.cache
            .iter()
            .filter(|(_, cached)| !cached.is_expired())
            .map(|((provider_id, capability), cached)| {
                (provider_id.as_str(), *capability, &cached.models)
            })
    }

    /// Load cache from disk into memory.
    fn load_from_disk(&mut self) {
        let path = match &self.cache_file {
            Some(p) => p.clone(),
            None => return,
        };

        if !path.exists() {
            tracing::debug!("No discovery cache file at {:?}", path);
            return;
        }

        let data = match std::fs::read_to_string(&path) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("Failed to read discovery cache file: {}", e);
                return;
            }
        };

        let file_cache: FileCacheData = match serde_json::from_str(&data) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    "Failed to parse discovery cache file: {}. Deleting stale cache.",
                    e
                );
                let _ = std::fs::remove_file(&path);
                return;
            }
        };

        if file_cache.version != 1 {
            tracing::warn!("Unknown cache version {} — ignoring", file_cache.version);
            return;
        }

        let mut loaded = 0;
        for entry in file_cache.entries {
            // Use a long TTL for disk-loaded entries — they don't auto-expire.
            // The user controls refresh via settings.
            let ttl_secs = 365 * 24 * 3600; // ~1 year
            let key = (entry.provider_id, entry.capability);
            self.cache
                .insert(key, CachedModels::new(entry.models, ttl_secs));
            loaded += 1;
        }

        tracing::info!("Loaded {} discovery cache entries from {:?}", loaded, path);
    }

    /// Persist current cache to disk.
    fn save_to_disk(&self) {
        let path = match &self.cache_file {
            Some(p) => p.clone(),
            None => return,
        };

        let now_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let entries: Vec<FileCacheEntry> = self
            .cache
            .iter()
            .map(|((provider_id, capability), cached)| FileCacheEntry {
                provider_id: provider_id.clone(),
                capability: *capability,
                models: cached.models.clone(),
                cached_at_unix: now_unix,
            })
            .collect();

        let file_cache = FileCacheData {
            version: 1,
            entries,
        };

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        match serde_json::to_string_pretty(&file_cache) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json) {
                    tracing::warn!("Failed to write discovery cache: {}", e);
                } else {
                    tracing::debug!("Saved discovery cache to {:?}", path);
                }
            }
            Err(e) => {
                tracing::warn!("Failed to serialize discovery cache: {}", e);
            }
        }
    }

    /// Delete the cache file from disk.
    fn delete_disk_cache(&self) {
        if let Some(path) = &self.cache_file {
            if path.exists() {
                if let Err(e) = std::fs::remove_file(path) {
                    tracing::warn!("Failed to delete discovery cache file: {}", e);
                } else {
                    tracing::debug!("Deleted discovery cache at {:?}", path);
                }
            }
        }
    }
}

impl Default for DiscoveryCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::config::{HttpMethod, RequestTemplate, ResponseTemplate, ResponseType};
    use std::collections::HashMap as StdHashMap;

    fn create_test_model(id: &str) -> ModelConfig {
        ModelConfig {
            id: id.to_string(),
            name: format!("Test Model {}", id),
            description: "Test model".to_string(),
            endpoint: "/test".to_string(),
            method: HttpMethod::POST,
            request: RequestTemplate {
                headers: StdHashMap::new(),
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
        }
    }

    #[test]
    fn test_cache_insert_and_get() {
        let mut cache = DiscoveryCache::new();
        let models = vec![create_test_model("model-1")];

        cache.insert(
            "test-provider".to_string(),
            ProviderCapability::TextToImage,
            models.clone(),
            3600,
        );

        let cached = cache.get("test-provider", ProviderCapability::TextToImage);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().len(), 1);
        assert_eq!(cached.unwrap()[0].id, "model-1");
    }

    #[test]
    fn test_cache_expiry() {
        let mut cache = DiscoveryCache::new();
        let models = vec![create_test_model("model-1")];

        // Insert with 0 second TTL (immediately expired)
        cache.insert(
            "test-provider".to_string(),
            ProviderCapability::TextToImage,
            models,
            0,
        );

        // Give it a moment to expire
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Should return None because entry is expired
        let cached = cache.get("test-provider", ProviderCapability::TextToImage);
        assert!(cached.is_none());
    }

    #[test]
    fn test_cache_invalidate() {
        let mut cache = DiscoveryCache::new();
        let models = vec![create_test_model("model-1")];

        cache.insert(
            "test-provider".to_string(),
            ProviderCapability::TextToImage,
            models,
            3600,
        );

        assert!(cache
            .get("test-provider", ProviderCapability::TextToImage)
            .is_some());

        cache.invalidate("test-provider", ProviderCapability::TextToImage);

        assert!(cache
            .get("test-provider", ProviderCapability::TextToImage)
            .is_none());
    }

    #[test]
    fn test_cache_clear() {
        let mut cache = DiscoveryCache::new();

        cache.insert(
            "provider-1".to_string(),
            ProviderCapability::TextToImage,
            vec![create_test_model("model-1")],
            3600,
        );

        cache.insert(
            "provider-2".to_string(),
            ProviderCapability::ImageTo3D,
            vec![create_test_model("model-2")],
            3600,
        );

        assert_eq!(cache.len(), 2);

        cache.clear();

        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cache_age() {
        let models = vec![create_test_model("model-1")];
        let cached = CachedModels::new(models, 3600);

        assert!(cached.age().as_secs() < 1);
        assert!(!cached.is_expired());
    }

    #[test]
    fn test_file_backed_cache_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("discovery_cache.json");

        // Create and populate cache
        {
            let mut cache = DiscoveryCache::with_file(cache_path.clone());
            cache.insert(
                "test-provider".to_string(),
                ProviderCapability::TextToImage,
                vec![create_test_model("model-1"), create_test_model("model-2")],
                3600,
            );
            cache.insert(
                "test-provider".to_string(),
                ProviderCapability::ImageTo3D,
                vec![create_test_model("model-3d")],
                3600,
            );
        }

        // File should exist
        assert!(cache_path.exists());

        // Load from file into new cache
        let cache = DiscoveryCache::with_file(cache_path);
        assert_eq!(cache.len(), 2);

        let text_models = cache.get("test-provider", ProviderCapability::TextToImage);
        assert!(text_models.is_some());
        assert_eq!(text_models.unwrap().len(), 2);
        assert_eq!(text_models.unwrap()[0].id, "model-1");

        let models_3d = cache.get("test-provider", ProviderCapability::ImageTo3D);
        assert!(models_3d.is_some());
        assert_eq!(models_3d.unwrap().len(), 1);
    }

    #[test]
    fn test_file_cache_clear_deletes_file() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("discovery_cache.json");

        let mut cache = DiscoveryCache::with_file(cache_path.clone());
        cache.insert(
            "test-provider".to_string(),
            ProviderCapability::TextToImage,
            vec![create_test_model("model-1")],
            3600,
        );
        assert!(cache_path.exists());

        cache.clear();
        assert!(!cache_path.exists());
        assert!(cache.is_empty());
    }

    #[test]
    fn test_has_models() {
        let mut cache = DiscoveryCache::new();
        assert!(!cache.has_models());

        cache.insert(
            "test-provider".to_string(),
            ProviderCapability::TextToImage,
            vec![create_test_model("model-1")],
            3600,
        );
        assert!(cache.has_models());
    }
}
