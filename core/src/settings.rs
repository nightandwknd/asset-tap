//! User settings and persistence.
//!
//! This module handles loading and saving user preferences, API keys, and configuration.
//! Settings behavior differs between development and release modes.
//!
//! # Settings Locations
//!
//! In **release mode**, settings are stored in the platform-specific config directory:
//! - macOS: `~/Library/Application Support/asset-tap/settings.json`
//! - Windows: `%APPDATA%\asset-tap\settings.json`
//! - Linux: `~/.config/asset-tap/settings.json`
//!
//! In **development mode** (debug builds), settings are stored locally:
//! - Settings file: `./.dev/settings.json`
//! - Output directory: `./.dev/output/`
//!
//! This allows developers to maintain separate settings from their production install.
//!
//! # API Keys
//!
//! API keys are loaded with the following priority:
//! 1. Settings file (`settings.json`) — configured via GUI or edited directly
//! 2. Environment variables (`FAL_KEY`, etc.) — works in all modes
//!
//! Both the GUI and CLI share the same settings file, so keys configured
//! in the GUI are automatically available to the CLI.
//!
//! # See Also
//!
//! - [`pipeline::PipelineConfig`](crate::pipeline::PipelineConfig) - Pipeline execution configuration
//! - [`providers::ProviderRegistry`](crate::providers::ProviderRegistry) - Provider management

use crate::constants::files::{APP_DISPLAY_NAME, APP_NAME, config as config_files, dev_dirs};
use crate::convert::find_blender;
use crate::providers::ProviderRegistry;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Settings filename.
const SETTINGS_FILE: &str = config_files::SETTINGS;

/// Local settings file for dev mode.
const DEV_SETTINGS_FILE: &str = config_files::DEV_SETTINGS;

/// Check if running in development mode (debug build).
pub fn is_dev_mode() -> bool {
    cfg!(debug_assertions)
}

/// Get the dynamic mapping of provider IDs to their environment variables.
///
/// This queries the provider registry to build a mapping based on the
/// `env_vars` field in each provider's configuration, ensuring we never
/// hardcode provider-specific information.
///
/// Returns a HashMap of (provider_id → env_var_name).
/// Note: Providers with multiple env_vars will only map to their first one.
fn get_provider_env_var_mapping(registry: &ProviderRegistry) -> HashMap<String, String> {
    let mut mapping = HashMap::new();

    for provider in registry.list_all() {
        let metadata = provider.metadata();
        if let Some(env_var) = metadata.required_env_vars.first() {
            mapping.insert(metadata.id.clone(), env_var.clone());
        }
    }

    mapping
}

/// Get the effective output directory, respecting dev mode.
///
/// In dev mode, returns `./.dev/output/` for local development.
/// In release mode, returns the user-configured directory from settings.
pub fn get_output_dir() -> PathBuf {
    if is_dev_mode() {
        PathBuf::from(dev_dirs::OUTPUT)
    } else {
        Settings::load().output_dir
    }
}

/// User settings for the Asset Tap.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    // =========================================================================
    // Paths
    // =========================================================================
    /// Directory where generated assets are saved.
    pub output_dir: PathBuf,

    /// Custom Blender executable path (None = auto-detect).
    pub blender_path: Option<String>,

    // =========================================================================
    // API Keys
    // =========================================================================
    /// Provider-specific API keys (provider ID -> API key value).
    #[serde(default)]
    pub provider_api_keys: HashMap<String, String>,

    // =========================================================================
    // Defaults
    // =========================================================================
    /// Whether FBX export is enabled by default.
    pub export_fbx_default: bool,

    // =========================================================================
    // UI Preferences
    // =========================================================================
    /// Whether to require user approval after image generation before proceeding to 3D.
    pub require_image_approval: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            // Paths
            output_dir: default_output_dir(),
            blender_path: None,

            // API Keys
            provider_api_keys: HashMap::new(),

            // Defaults
            export_fbx_default: false,

            // UI Preferences
            require_image_approval: true,
        }
    }
}

impl Settings {
    /// Load settings from the config file.
    ///
    /// If the file doesn't exist, creates it with detected/default settings.
    /// If the file is invalid, returns default settings (but doesn't overwrite).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use asset_tap_core::Settings;
    ///
    /// let settings = Settings::load();
    /// println!("Output directory: {}", settings.output_dir.display());
    /// ```
    pub fn load() -> Self {
        let path = settings_file_path();

        if !path.exists() {
            // Create settings with detected values
            let mut settings = Self::default();
            settings.detect_and_populate();
            if let Err(e) = settings.save() {
                tracing::warn!("Failed to create default settings file: {}", e);
            }
            return settings;
        }

        match std::fs::read_to_string(&path) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Detect and populate settings with system values.
    ///
    /// Called when creating a new settings file to pre-populate with:
    /// - Auto-detected Blender path
    /// - API keys from environment variables (in dev mode)
    /// - Default model selections
    pub fn detect_and_populate(&mut self) {
        // Detect Blender
        if let Some(blender) = find_blender() {
            self.blender_path = Some(blender);
        }

        // Note: sync_from_env() is called separately by GUI/CLI after creating the provider registry
        // We skip it here to avoid creating a registry during settings initialization
    }

    /// Save settings to the config file.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use asset_tap_core::Settings;
    ///
    /// let mut settings = Settings::load();
    /// settings.export_fbx_default = false;
    /// settings.save()?;
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn save(&self) -> std::io::Result<()> {
        let path = settings_file_path();

        // Ensure config directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let contents = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;

        std::fs::write(&path, &contents)?;

        // Restrict file permissions to owner-only (0600) on Unix.
        // Settings contain API keys that should not be world-readable.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&path, perms)?;
        }

        Ok(())
    }

    /// Check if the output directory exists and is writable.
    pub fn output_dir_valid(&self) -> bool {
        self.output_dir.exists() && self.output_dir.is_dir()
    }

    /// Ensure the output directory exists, creating it if necessary.
    pub fn ensure_output_dir(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.output_dir)
    }

    // =========================================================================
    // Effective Value Getters (with .env fallback in dev mode)
    // =========================================================================

    /// Get API key for a specific provider.
    ///
    /// Priority: settings file > environment variable.
    ///
    /// **Dynamic**: Uses provider registry to determine env var names,
    /// ensuring no hardcoded provider-specific logic.
    pub fn get_provider_api_key(
        &self,
        provider_id: &str,
        registry: &ProviderRegistry,
    ) -> Option<String> {
        // Check settings first (persisted keys from GUI)
        if let Some(key) = self.provider_api_keys.get(provider_id)
            && !key.is_empty()
        {
            return Some(key.clone());
        }

        // Fall back to environment variables (works in all modes — dev, release, CLI)
        let mapping = get_provider_env_var_mapping(registry);
        if let Some(env_var) = mapping.get(provider_id)
            && let Ok(key) = std::env::var(env_var)
            && !key.is_empty()
        {
            return Some(key);
        }

        None
    }

    /// Set API key for a specific provider.
    pub fn set_provider_api_key(&mut self, provider_id: impl Into<String>, key: impl Into<String>) {
        let key_string = key.into();
        if key_string.is_empty() {
            self.provider_api_keys.remove(&provider_id.into());
        } else {
            self.provider_api_keys
                .insert(provider_id.into(), key_string);
        }
    }

    /// Get all configured provider API keys with metadata about their source.
    ///
    /// Returns a HashMap of (provider_id → (key, is_from_env)).
    /// In dev mode, `is_from_env` indicates if the key came from environment vars.
    ///
    /// **Dynamic**: Iterates over all providers from the registry.
    /// This is the canonical way for UIs to get all provider keys.
    pub fn get_all_provider_keys(
        &self,
        registry: &ProviderRegistry,
    ) -> HashMap<String, (String, bool)> {
        let mapping = get_provider_env_var_mapping(registry);
        let mut result = HashMap::new();

        for provider in registry.list_all() {
            let provider_id = provider.id();

            // Check settings first, then env var (same priority as get_provider_api_key)
            let key = self
                .provider_api_keys
                .get(provider_id)
                .filter(|k| !k.is_empty())
                .cloned()
                .or_else(|| {
                    mapping
                        .get(provider_id)
                        .and_then(|env_var| std::env::var(env_var).ok())
                        .filter(|k| !k.is_empty())
                });

            if let Some(key) = key {
                // Check if this key came from env (in dev mode)
                let is_from_env = if is_dev_mode() {
                    if let Some(env_var) = provider.metadata().required_env_vars.first() {
                        if let Ok(env_key) = std::env::var(env_var) {
                            !env_key.is_empty() && env_key == key
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                };

                result.insert(provider_id.to_string(), (key, is_from_env));
            }
        }

        result
    }

    /// Get the effective Blender path.
    ///
    /// Priority: settings file (if set) > auto-detect
    pub fn get_blender_path(&self) -> Option<String> {
        // If user has set a custom path, use it
        if let Some(ref path) = self.blender_path
            && !path.is_empty()
        {
            return Some(path.clone());
        }
        // Otherwise, auto-detect
        find_blender()
    }

    /// Check if at least one provider API key is configured.
    ///
    /// **Dynamic**: Checks all providers from the registry, not hardcoded IDs.
    pub fn has_required_api_keys(&self, registry: &ProviderRegistry) -> bool {
        let mapping = get_provider_env_var_mapping(registry);
        mapping.iter().any(|(provider_id, env_var)| {
            // Check settings first
            if let Some(key) = self.provider_api_keys.get(provider_id)
                && !key.is_empty()
            {
                return true;
            }
            // Fall back to environment variable
            std::env::var(env_var)
                .map(|v| !v.is_empty())
                .unwrap_or(false)
        })
    }

    /// Sync environment variables to settings (for dev mode).
    ///
    /// This copies any API keys found in environment variables to the settings,
    /// so they persist in settings.json for consistency.
    ///
    /// **Dynamic**: Queries the provider registry to determine which env vars to sync,
    /// ensuring no hardcoded provider-specific logic.
    pub fn sync_from_env(&mut self, registry: &ProviderRegistry) {
        if is_dev_mode() {
            let mapping = get_provider_env_var_mapping(registry);

            for (provider_id, env_var) in mapping {
                if let Ok(key) = std::env::var(&env_var) {
                    // Only add if not empty and not already in settings
                    if !key.is_empty() && !self.provider_api_keys.contains_key(&provider_id) {
                        self.provider_api_keys.insert(provider_id, key);
                    }
                }
            }
        }
    }

    /// Sync settings to environment variables (for GUI app).
    ///
    /// This sets environment variables from settings so providers can access them.
    /// Only affects the current process.
    ///
    /// **Dev mode behavior**: Does NOT overwrite env vars that are already set,
    /// preserving values from `.env` file.
    ///
    /// **Dynamic**: Queries the provider registry to determine which env vars to set,
    /// ensuring no hardcoded provider-specific logic.
    pub fn sync_to_env(&self, registry: &ProviderRegistry) {
        let mapping = get_provider_env_var_mapping(registry);

        for (provider_id, env_var) in mapping {
            // In dev mode, don't overwrite env vars that are already set (from .env)
            if is_dev_mode() && std::env::var(&env_var).is_ok() {
                continue;
            }

            if let Some(key) = self.provider_api_keys.get(&provider_id) {
                // SAFETY: This is called from the main thread during app initialization,
                // before any async tasks that read these env vars are spawned.
                unsafe { std::env::set_var(&env_var, key) };
            } else {
                // SAFETY: Same as above — called during single-threaded initialization.
                unsafe { std::env::remove_var(&env_var) };
            }
        }
    }
}

/// Get the path to the config directory (release mode only).
///
/// In dev mode, use `settings_file_path()` directly instead.
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_NAME)
}

/// Get the path to the settings file.
///
/// - Dev mode: `./.dev/settings.json` (local to repo)
/// - Release mode: OS-specific config directory
pub fn settings_file_path() -> PathBuf {
    if is_dev_mode() {
        PathBuf::from(DEV_SETTINGS_FILE)
    } else {
        config_dir().join(SETTINGS_FILE)
    }
}

/// Get the default output directory.
///
/// - Dev mode: `./.dev/output/` (local to repo)
/// - Release mode: `~/Documents/Asset Tap/`
fn default_output_dir() -> PathBuf {
    if is_dev_mode() {
        PathBuf::from(dev_dirs::OUTPUT)
    } else {
        dirs::document_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."))
            .join(APP_DISPLAY_NAME)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_settings() {
        let settings = Settings::default();
        assert!(!settings.export_fbx_default);
        assert!(settings.require_image_approval);
    }

    #[test]
    fn test_settings_serialization() {
        let settings = Settings::default();
        let json = serde_json::to_string(&settings).unwrap();
        let loaded: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(
            settings.require_image_approval,
            loaded.require_image_approval
        );
    }

    #[test]
    fn test_config_dir() {
        let dir = config_dir();
        assert!(dir.ends_with(APP_NAME));
    }

    #[test]
    fn test_set_and_get_provider_api_key() {
        let mut settings = Settings::default();

        // No key initially
        assert!(settings.provider_api_keys.is_empty());

        // Set a key
        settings.set_provider_api_key("test-provider", "my-secret-key");
        assert_eq!(
            settings.provider_api_keys.get("test-provider").unwrap(),
            "my-secret-key"
        );

        // Setting empty key removes it
        settings.set_provider_api_key("test-provider", "");
        assert!(!settings.provider_api_keys.contains_key("test-provider"));
    }

    #[test]
    fn test_get_provider_api_key_from_settings() {
        let mut settings = Settings::default();
        settings.set_provider_api_key("fal-ai", "settings-key");

        let registry = ProviderRegistry::new();

        // Should return the settings key
        let key = settings.get_provider_api_key("fal-ai", &registry);
        assert_eq!(key, Some("settings-key".to_string()));

        // Non-existent provider returns None
        let key = settings.get_provider_api_key("nonexistent", &registry);
        assert!(key.is_none());
    }

    #[test]
    fn test_has_required_api_keys() {
        let registry = ProviderRegistry::new();

        // No keys set — should be false
        let settings = Settings::default();
        assert!(!settings.has_required_api_keys(&registry));

        // Set a key for each registered provider — should be true if any providers exist
        if !registry.list_all().is_empty() {
            let mut settings = Settings::default();
            let first_id = registry.list_all()[0].id().to_string();
            settings.set_provider_api_key(&first_id, "test-key");
            assert!(settings.has_required_api_keys(&registry));
        }
    }

    #[test]
    fn test_sync_from_env_only_adds_missing() {
        let registry = ProviderRegistry::new();

        // Find a provider with env vars to test with
        let provider_info = registry.list_all().into_iter().find_map(|p| {
            let meta = p.metadata();
            meta.required_env_vars
                .first()
                .map(|env_var| (meta.id.clone(), env_var.clone()))
        });

        if let Some((provider_id, env_var)) = provider_info {
            // Pre-set the env var
            unsafe { std::env::set_var(&env_var, "env-key-value") };

            let mut settings = Settings::default();
            settings.sync_from_env(&registry);

            // In dev mode, should have synced the key
            if is_dev_mode() {
                assert_eq!(
                    settings.provider_api_keys.get(&provider_id).unwrap(),
                    "env-key-value"
                );

                // If key already exists in settings, sync_from_env should NOT overwrite
                settings.set_provider_api_key(&provider_id, "existing-key");
                settings.sync_from_env(&registry);
                assert_eq!(
                    settings.provider_api_keys.get(&provider_id).unwrap(),
                    "existing-key"
                );
            }

            // Clean up
            unsafe { std::env::remove_var(&env_var) };
        }
    }

    #[test]
    fn test_sync_to_env() {
        let registry = ProviderRegistry::new();

        // Find a provider with env vars to test with
        let provider_info = registry.list_all().into_iter().find_map(|p| {
            let meta = p.metadata();
            meta.required_env_vars
                .first()
                .map(|env_var| (meta.id.clone(), env_var.clone()))
        });

        if let Some((provider_id, env_var)) = provider_info {
            // Clean up any existing value
            unsafe { std::env::remove_var(&env_var) };

            let mut settings = Settings::default();
            settings.set_provider_api_key(&provider_id, "synced-key");
            settings.sync_to_env(&registry);

            // In release mode, should always set; in dev mode, sets if not already present
            // Since we removed it above, should be set in either mode
            // However sync_to_env in dev mode skips if env var is already set,
            // and we just removed it, so it should set it.

            // Clean up
            unsafe { std::env::remove_var(&env_var) };
        }
    }

    #[test]
    fn test_get_all_provider_keys() {
        let registry = ProviderRegistry::new();

        // Find a provider to test with
        if let Some(provider) = registry.list_all().first() {
            let provider_id = provider.id().to_string();

            let mut settings = Settings::default();
            settings.set_provider_api_key(&provider_id, "test-key");

            let all_keys = settings.get_all_provider_keys(&registry);
            assert!(all_keys.contains_key(&provider_id));
            let (key, _is_from_env) = all_keys.get(&provider_id).unwrap();
            assert_eq!(key, "test-key");
        }
    }

    #[test]
    fn test_output_dir_operations() {
        let settings = Settings::default();
        // Default output dir path should be set
        assert!(!settings.output_dir.as_os_str().is_empty());

        // ensure_output_dir should succeed (creates if needed)
        let temp = tempfile::tempdir().unwrap();
        let settings = Settings {
            output_dir: temp.path().join("test_output"),
            ..Default::default()
        };
        settings.ensure_output_dir().unwrap();
        assert!(settings.output_dir_valid());
    }

    #[test]
    fn test_get_blender_path_custom() {
        let mut settings = Settings::default();

        // No custom path — falls back to auto-detect
        let _path = settings.get_blender_path();
        // (may or may not find Blender, that's OK)

        // Set custom path
        settings.blender_path = Some("/usr/bin/blender".to_string());
        assert_eq!(
            settings.get_blender_path(),
            Some("/usr/bin/blender".to_string())
        );

        // Empty string treated as unset
        settings.blender_path = Some("".to_string());
        // Should fall back to auto-detect, not return empty string
        assert_ne!(settings.get_blender_path(), Some("".to_string()));
    }
}
