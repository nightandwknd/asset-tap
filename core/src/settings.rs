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
//! # Pushing keys into the process environment
//!
//! Providers read API keys from `std::env::var(...)`, not from settings
//! directly, so on startup we need to copy any settings-side keys into the
//! process environment. There are two flavors:
//!
//! - [`Settings::sync_to_env`] — **set-only**. Copies non-empty keys from
//!   settings into env, but never *removes* env vars. Use this on startup so
//!   pre-existing values from `.env`, the shell, or `apply_mock_mode`'s
//!   dummy-key injection survive.
//! - [`Settings::sync_to_env_authoritative`] — **set-or-remove**. Same as
//!   above, but additionally removes env vars whose corresponding setting is
//!   missing or empty. Use this only from the GUI settings dialog's save
//!   handler, where the user has explicitly cleared a key and expects it to
//!   stop taking effect immediately.
//!
//! Calling the authoritative variant on startup is wrong: it will stomp
//! every legitimate value the user expected to inherit from elsewhere.
//!
//! # Loading and load failures
//!
//! [`Settings::load_with_status`] is the canonical entry point. It returns a
//! [`LoadStatus`] alongside the parsed settings so the caller can surface
//! parse failures, missing-file create failures, and corrupt-file quarantines
//! to the user (the GUI as a startup toast, the CLI as a stderr warning).
//! The plain [`Settings::load`] is a convenience wrapper that discards the
//! status — only use it from contexts where you genuinely don't care about
//! the failure mode.
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
use std::path::{Path, PathBuf};

/// Settings filename.
const SETTINGS_FILE: &str = config_files::SETTINGS;

/// Local settings file for dev mode.
const DEV_SETTINGS_FILE: &str = config_files::DEV_SETTINGS;

/// Filesystem extension sidecar suffixes used by [`Settings::save_to`] and
/// [`Settings::load_from_with_status`]. These are passed to
/// [`std::path::Path::with_extension`], which replaces the entire existing
/// extension — so "json.tmp" produces `settings.json.tmp`, not
/// `settings.json.json.tmp`.
const TMP_EXT: &str = "json.tmp";
const BAK_EXT: &str = "json.bak";
/// Prefix used to rename a corrupt settings file aside for recovery. The
/// full sidecar filename is `<settings-filename>.corrupt-<unix_timestamp>`.
const CORRUPT_PREFIX: &str = "corrupt-";

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

/// Outcome of a [`Settings::load_with_status`] call.
///
/// The GUI and CLI use this to surface corruption and other load failures
/// to the user. The plain log lines alone aren't visible to a non-technical
/// user, and silently swallowing failure modes is how API keys go missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadStatus {
    /// File was loaded cleanly, or didn't exist and we created a fresh one.
    Ok,
    /// File didn't exist and we tried to create it with defaults, but the
    /// write failed. The session has defaults in memory but **no settings
    /// file on disk** — anything the user enters this session will be lost
    /// unless the underlying problem (likely permissions or full disk) is
    /// resolved before the next save.
    InitialCreateFailed {
        settings_path: PathBuf,
        error: String,
    },
    /// File existed but couldn't be parsed. The original was moved to the
    /// contained path for recovery.
    RecoveredFromCorrupt { quarantined_to: PathBuf },
    /// File existed but couldn't be parsed *and* couldn't be moved aside.
    /// It's still sitting at `settings_path`. The next save will overwrite
    /// it (and the corrupt bytes will live in `.bak` for one generation).
    CorruptAndInPlace { settings_path: PathBuf },
    /// File existed but couldn't even be read (permissions, I/O error, etc.).
    UnreadableFile { settings_path: PathBuf },
}

impl Settings {
    /// Load settings from the config file.
    ///
    /// - If the file doesn't exist, creates it with detected/default values.
    /// - If the file exists but can't be parsed, the corrupt file is renamed
    ///   to `settings.json.corrupt-<unix_timestamp>` so the user can recover
    ///   it by hand, a loud error is logged, and defaults are returned. The
    ///   next save will write a fresh `settings.json`.
    /// - If the file exists and parses, missing fields are filled with
    ///   `#[serde(default)]` — additive schema changes upgrade transparently.
    ///
    /// This is a convenience wrapper around [`Self::load_with_status`] that
    /// discards the load status. Callers who need to react to corruption
    /// (e.g., show a UI warning) should use `load_with_status` instead.
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
        Self::load_with_status().0
    }

    /// Load settings and report the outcome.
    ///
    /// See [`LoadStatus`] for the possible results. Use this variant when you
    /// want to surface parse failures to the user.
    pub fn load_with_status() -> (Self, LoadStatus) {
        Self::load_from_with_status(&settings_file_path())
    }

    /// Test-visible inner implementation of [`Self::load_with_status`].
    pub(crate) fn load_from_with_status(path: &Path) -> (Self, LoadStatus) {
        if !path.exists() {
            let mut settings = Self::default();
            settings.detect_and_populate();
            return match settings.save_to(path) {
                Ok(()) => (settings, LoadStatus::Ok),
                Err(e) => {
                    tracing::warn!("Failed to create default settings file: {}", e);
                    (
                        settings,
                        LoadStatus::InitialCreateFailed {
                            settings_path: path.to_path_buf(),
                            error: e.to_string(),
                        },
                    )
                }
            };
        }

        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(
                    "Failed to read settings file {:?}: {}. Falling back to defaults.",
                    path,
                    e
                );
                return (
                    Self::default(),
                    LoadStatus::UnreadableFile {
                        settings_path: path.to_path_buf(),
                    },
                );
            }
        };

        match serde_json::from_str::<Settings>(&contents) {
            Ok(settings) => (settings, LoadStatus::Ok),
            Err(e) => {
                // Preserve the unreadable file so the user can recover it.
                let quarantine = quarantine_path(path);
                match std::fs::rename(path, &quarantine) {
                    Ok(()) => {
                        tracing::error!(
                            "settings.json is corrupt and could not be parsed: {}. \
                             Your original file has been preserved at {:?}. \
                             A fresh settings.json with defaults will be created on next save.",
                            e,
                            quarantine
                        );
                        (
                            Self::default(),
                            LoadStatus::RecoveredFromCorrupt {
                                quarantined_to: quarantine,
                            },
                        )
                    }
                    Err(rename_err) => {
                        // Couldn't move the file — leave it in place and still return
                        // defaults. Don't save() over it: that would clobber the original.
                        tracing::error!(
                            "settings.json is corrupt ({}) and could not be quarantined to \
                             {:?} ({}). The file has been left in place — inspect it \
                             manually. Running with defaults for this session.",
                            e,
                            quarantine,
                            rename_err
                        );
                        (
                            Self::default(),
                            LoadStatus::CorruptAndInPlace {
                                settings_path: path.to_path_buf(),
                            },
                        )
                    }
                }
            }
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

    /// Save settings to the config file atomically.
    ///
    /// Writes to `settings.json.tmp`, fsyncs, then renames over the target.
    /// A crash at any point leaves either the old file or the new file fully
    /// written — never a half-written mix. Before the rename, the previous
    /// `settings.json` (if any) is copied to `settings.json.bak`, overwriting
    /// any prior backup. One `.bak` generation is kept; that's enough to
    /// recover from a single bad save without unbounded disk use.
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
        self.save_to(&settings_file_path())
    }

    /// Test-visible inner implementation of [`Self::save`]. See [`Self::load_from`].
    pub(crate) fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let contents = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;

        // 1. Write new contents to a tmp file in the same directory so the
        //    subsequent rename is atomic (same filesystem guaranteed).
        let tmp_path = path.with_extension(TMP_EXT);
        {
            use std::io::Write as _;
            let mut tmp = std::fs::File::create(&tmp_path)?;
            tmp.write_all(contents.as_bytes())?;
            // fsync the data to disk before rename — without this, a crash
            // between write() and rename() can leave an empty tmp file that
            // the next rename promotes to the real settings.json.
            tmp.sync_all()?;
        }

        // Restrict file permissions on the tmp file *before* the rename so
        // the final file is never world-readable even for an instant.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&tmp_path, perms)?;
        }

        // 2. Back up the existing file before stomping it.
        if path.exists() {
            let bak_path = path.with_extension(BAK_EXT);
            if let Err(e) = std::fs::copy(path, &bak_path) {
                // Non-fatal: we'd rather save the new settings than refuse
                // because the backup failed. Log it and continue.
                tracing::warn!(
                    "Failed to back up existing settings to {:?}: {}",
                    bak_path,
                    e
                );
            }
        }

        // 3. Atomic rename into place.
        std::fs::rename(&tmp_path, path)?;

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

    /// Push API keys from settings into the process environment.
    ///
    /// This is the **startup** variant: it only *sets* env vars from settings
    /// keys, it never *removes* them. Pre-existing env vars (from `.env`,
    /// the shell, or `apply_mock_mode` injecting dummy keys) are preserved
    /// when the corresponding settings entry is empty or missing.
    ///
    /// **Dev mode behavior**: Does NOT overwrite env vars that are already
    /// set, preserving values from `.env` file.
    ///
    /// Use [`Self::sync_to_env_authoritative`] from the GUI's settings save
    /// path if you want "settings is the only source of truth" semantics —
    /// e.g., when the user explicitly clears a key and expects the env to
    /// reflect that.
    ///
    /// **Dynamic**: Queries the provider registry to determine which env
    /// vars to set, ensuring no hardcoded provider-specific logic.
    pub fn sync_to_env(&self, registry: &ProviderRegistry) {
        self.sync_to_env_inner(registry, /* authoritative */ false, is_dev_mode());
    }

    /// Push API keys from settings into the process environment **and**
    /// remove env vars whose corresponding setting is missing or empty.
    ///
    /// Use this from the GUI settings dialog's save handler when the user
    /// has explicitly cleared a key and expects the running process to stop
    /// using whatever was previously in env. **Do not call this at startup**
    /// — it will stomp legitimate values from `.env`, the shell, or mock
    /// mode that the user never explicitly removed.
    pub fn sync_to_env_authoritative(&self, registry: &ProviderRegistry) {
        self.sync_to_env_inner(registry, /* authoritative */ true, is_dev_mode());
    }

    /// Pure-ish inner implementation of [`Self::sync_to_env`] /
    /// [`Self::sync_to_env_authoritative`].
    ///
    /// `authoritative` controls whether missing/empty settings entries should
    /// also remove the corresponding env var. `dev_mode` controls whether
    /// pre-existing env vars are protected from overwrite/removal — in dev
    /// mode they always are, so `.env` values aren't stomped during local
    /// iteration. The flag is passed in (rather than read from
    /// [`is_dev_mode`]) so unit tests can exercise both branches without
    /// rebuilding for a different cargo profile.
    ///
    /// Still mutates global state via `set_var` / `remove_var`, so callers
    /// must ensure they're on the main thread before any async task that
    /// reads these env vars has spawned.
    pub(crate) fn sync_to_env_inner(
        &self,
        registry: &ProviderRegistry,
        authoritative: bool,
        dev_mode: bool,
    ) {
        let mapping = get_provider_env_var_mapping(registry);

        for (provider_id, env_var) in mapping {
            // Dev-mode protection: a value already in env (from .env, the
            // shell, etc.) is sacred during dev iteration. Skip both set and
            // remove paths in that case.
            if dev_mode && std::env::var(&env_var).is_ok() {
                continue;
            }

            match self.provider_api_keys.get(&provider_id) {
                Some(key) if !key.is_empty() => {
                    // SAFETY: called from main-thread initialization (startup
                    // sync) or a synchronous GUI save handler. No async task
                    // is reading these env vars concurrently.
                    unsafe { std::env::set_var(&env_var, key) };
                }
                _ if authoritative => {
                    // SAFETY: same as above.
                    unsafe { std::env::remove_var(&env_var) };
                }
                _ => {
                    // Non-authoritative: leave the env var alone. It may
                    // have been set by .env, the shell, or mock mode.
                }
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

/// Build a quarantine path for a corrupt settings file.
///
/// The result sits in the same directory as the original with a
/// `.corrupt-<unix_timestamp>` suffix, so multiple corrupt files can coexist
/// and the user can identify them chronologically. If two quarantines happen
/// within the same clock second (rare, but possible in tests or rapid retries)
/// the [`crate::config::find_unused_with_counter_suffix`] helper appends
/// `-1`, `-2`, ... so neither overwrites the other.
///
/// The timestamp is best-effort: if the system clock is unreadable we fall
/// back to `.corrupt-unknown`, again with the collision counter. Clock failure
/// is vanishingly rare but we still don't want to lose data to it.
fn quarantine_path(settings_path: &Path) -> PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let filename = settings_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(SETTINGS_FILE);

    let name = format!("{filename}.{CORRUPT_PREFIX}{ts}");
    let base = match settings_path.parent() {
        Some(p) => p.join(name),
        None => PathBuf::from(name),
    };
    crate::config::find_unused_with_counter_suffix(base)
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

    /// Thin wrapper so existing tests can say `load_from(&path)` instead of
    /// `Settings::load_from_with_status(&path).0`. Equivalent — just less noise.
    impl Settings {
        fn load_from(path: &Path) -> Self {
            Self::load_from_with_status(path).0
        }
    }

    /// Shared filename prefix for quarantined corrupt files in the temp dirs
    /// used by these tests. Built from the real module consts so renames
    /// elsewhere in this file will ripple through automatically.
    fn corrupt_prefix_for(filename: &str) -> String {
        format!("{filename}.{CORRUPT_PREFIX}")
    }

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

            // In dev mode, sync_to_env skips if env var is already set, but
            // we removed it above so it should set it. In release mode, it
            // always sets (when settings has a non-empty value).
            assert_eq!(
                std::env::var(&env_var).ok().as_deref(),
                Some("synced-key"),
                "sync_to_env should populate env from settings"
            );

            // Clean up
            unsafe { std::env::remove_var(&env_var) };
        }
    }

    /// Regression guard: `sync_to_env` (the non-authoritative variant used at
    /// startup) must NOT remove env vars when settings is empty for that key.
    /// Removing them stomps legitimate values from `.env`, the shell, or
    /// `apply_mock_mode`'s dummy-key injection — and that's exactly what
    /// happens to a user with a corrupt or freshly-defaulted settings file.
    #[test]
    fn test_sync_to_env_preserves_preexisting_env_when_settings_empty() {
        let registry = ProviderRegistry::new();

        let provider_info = registry.list_all().into_iter().find_map(|p| {
            let meta = p.metadata();
            meta.required_env_vars
                .first()
                .map(|v| (meta.id.clone(), v.clone()))
        });

        if let Some((_provider_id, env_var)) = provider_info {
            // Pretend something else (mock mode, .env, the shell) put a value
            // here before us.
            unsafe { std::env::set_var(&env_var, "preexisting-from-elsewhere") };

            // Default settings has no provider keys at all.
            let settings = Settings::default();
            settings.sync_to_env(&registry);

            // The pre-existing value must still be there.
            assert_eq!(
                std::env::var(&env_var).ok().as_deref(),
                Some("preexisting-from-elsewhere"),
                "sync_to_env must not remove env vars that aren't in settings"
            );

            unsafe { std::env::remove_var(&env_var) };
        }
    }

    /// Authoritative variant — used by the GUI's settings save dialog when
    /// the user has explicitly cleared a key. In release mode that env var
    /// SHOULD be removed even if `.env` or mock mode set it earlier.
    ///
    /// We call `sync_to_env_inner` directly with `dev_mode = false` so this
    /// test exercises the release branch even when running under
    /// `cargo test` (which uses the debug profile, where `is_dev_mode()`
    /// would return true and short-circuit the remove).
    #[test]
    fn test_sync_to_env_authoritative_removes_when_settings_empty() {
        let registry = ProviderRegistry::new();

        let provider_info = registry.list_all().into_iter().find_map(|p| {
            let meta = p.metadata();
            meta.required_env_vars
                .first()
                .map(|v| (meta.id.clone(), v.clone()))
        });

        if let Some((_provider_id, env_var)) = provider_info {
            unsafe { std::env::set_var(&env_var, "should-be-removed") };

            let settings = Settings::default();
            settings.sync_to_env_inner(
                &registry, /* authoritative */ true, /* dev_mode */ false,
            );

            assert!(
                std::env::var(&env_var).is_err(),
                "sync_to_env_authoritative should remove env vars not in settings"
            );

            // Defensive cleanup in case the assertion was loosened.
            unsafe { std::env::remove_var(&env_var) };
        }
    }

    /// Dev-mode protection: even the authoritative variant must NOT touch
    /// pre-existing env vars when `dev_mode = true`. The local `.env` file
    /// is the source of truth during dev iteration, not whatever the user
    /// happens to have in their `settings.json`.
    #[test]
    fn test_sync_to_env_authoritative_preserves_env_in_dev_mode() {
        let registry = ProviderRegistry::new();

        let provider_info = registry.list_all().into_iter().find_map(|p| {
            let meta = p.metadata();
            meta.required_env_vars
                .first()
                .map(|v| (meta.id.clone(), v.clone()))
        });

        if let Some((_provider_id, env_var)) = provider_info {
            unsafe { std::env::set_var(&env_var, "from-dev-env") };

            let settings = Settings::default();
            settings.sync_to_env_inner(
                &registry, /* authoritative */ true, /* dev_mode */ true,
            );

            assert_eq!(
                std::env::var(&env_var).ok().as_deref(),
                Some("from-dev-env"),
                "dev_mode = true must preserve pre-existing env even in the authoritative variant"
            );

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

    // =========================================================================
    // Load / save persistence and corruption handling
    // =========================================================================

    #[test]
    fn test_save_to_and_load_from_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");

        let mut original = Settings::default();
        original.set_provider_api_key("fal-ai", "k-round-trip");
        original.export_fbx_default = true;
        original.require_image_approval = false;

        original.save_to(&path).unwrap();
        let loaded = Settings::load_from(&path);

        assert_eq!(
            loaded.provider_api_keys.get("fal-ai").map(String::as_str),
            Some("k-round-trip")
        );
        assert!(loaded.export_fbx_default);
        assert!(!loaded.require_image_approval);
    }

    #[test]
    fn test_load_from_missing_file_creates_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert!(!path.exists());

        let _ = Settings::load_from(&path);
        assert!(
            path.exists(),
            "load_from should create the file when missing"
        );
    }

    #[test]
    fn test_save_to_backs_up_previous_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");

        let mut first = Settings::default();
        first.set_provider_api_key("fal-ai", "original-key");
        first.save_to(&path).unwrap();

        let mut second = Settings::default();
        second.set_provider_api_key("fal-ai", "new-key");
        second.save_to(&path).unwrap();

        // .bak contains the first version.
        let bak = path.with_extension(BAK_EXT);
        assert!(bak.exists(), ".bak should exist after a second save");
        let bak_contents = std::fs::read_to_string(&bak).unwrap();
        let bak_parsed: Settings = serde_json::from_str(&bak_contents).unwrap();
        assert_eq!(
            bak_parsed
                .provider_api_keys
                .get("fal-ai")
                .map(String::as_str),
            Some("original-key")
        );

        // Current file contains the second version.
        let loaded = Settings::load_from(&path);
        assert_eq!(
            loaded.provider_api_keys.get("fal-ai").map(String::as_str),
            Some("new-key")
        );
    }

    #[test]
    fn test_save_to_first_save_has_no_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");

        Settings::default().save_to(&path).unwrap();

        let bak = path.with_extension(BAK_EXT);
        assert!(
            !bak.exists(),
            "first save should not create a .bak — nothing to back up"
        );
    }

    #[test]
    fn test_save_to_leaves_no_tmp_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");

        Settings::default().save_to(&path).unwrap();

        let tmp = path.with_extension(TMP_EXT);
        assert!(
            !tmp.exists(),
            "a successful save should rename the tmp away, not leave it"
        );
    }

    #[test]
    fn test_load_from_corrupt_file_quarantines_and_returns_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");

        // Write garbage that serde_json can't parse.
        std::fs::write(&path, "this is not json {{{ ").unwrap();

        let loaded = Settings::load_from(&path);

        // Returned settings are the defaults, not whatever junk was in the file.
        assert_eq!(
            loaded.require_image_approval,
            Settings::default().require_image_approval
        );
        assert!(loaded.provider_api_keys.is_empty());

        // The corrupt file was quarantined, not deleted. The original settings.json
        // should be gone (moved), and exactly one .corrupt-* sibling should exist.
        assert!(
            !path.exists(),
            "original settings.json should have been renamed"
        );

        let prefix = corrupt_prefix_for("settings.json");
        let quarantined: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(&prefix))
            .collect();
        assert_eq!(
            quarantined.len(),
            1,
            "expected one quarantined file, found {:?}",
            quarantined
        );

        // The quarantined file contains the exact original bytes.
        let quarantined_path = dir.path().join(&quarantined[0]);
        assert_eq!(
            std::fs::read_to_string(&quarantined_path).unwrap(),
            "this is not json {{{ "
        );
    }

    /// File exists but `read_to_string` fails because we can't open it.
    /// We should get `UnreadableFile`, not `Ok` and not a quarantine attempt.
    #[cfg(unix)]
    #[test]
    fn test_load_from_with_status_unreadable_file() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        // Write valid JSON first so we can prove the failure isn't a parse error.
        std::fs::write(&path, br#"{"output_dir":"/tmp"}"#).unwrap();
        // Now strip read permission. We don't run tests as root, so this works.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();

        let (_settings, status) = Settings::load_from_with_status(&path);

        // Restore perms before assertions so a panic doesn't leave an
        // un-cleanable tempdir behind for tempfile.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        match status {
            LoadStatus::UnreadableFile { settings_path } => {
                assert_eq!(settings_path, path);
            }
            other => panic!("expected UnreadableFile, got {:?}", other),
        }
        // The original file is still in place — UnreadableFile must NOT touch it.
        assert!(path.exists());
    }

    /// File exists, parses as garbage, but rename-aside fails because the
    /// parent directory is non-writable. We should get `CorruptAndInPlace`
    /// and the original file should still be sitting at `path` afterwards.
    #[cfg(unix)]
    #[test]
    fn test_load_from_with_status_corrupt_and_in_place() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "this is not valid json").unwrap();

        // r-x on the parent: read_to_string can still open the file (the parent
        // is searchable), but rename can't create a new entry in the parent
        // (no write bit), so the quarantine rename fails.
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o500)).unwrap();

        let (_settings, status) = Settings::load_from_with_status(&path);

        // Restore perms so tempfile can clean up.
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();

        match status {
            LoadStatus::CorruptAndInPlace { settings_path } => {
                assert_eq!(settings_path, path);
            }
            other => panic!("expected CorruptAndInPlace, got {:?}", other),
        }
        // The corrupt original is still where we left it — load did not move
        // it (because rename failed) and did not save over it (because that
        // would clobber the original).
        assert!(path.exists());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "this is not valid json"
        );
    }

    /// Security claim: the saved settings file is mode 0600. API keys live in
    /// this file, so it must never be world-readable. The atomic-save path
    /// chmods the tmp file before the rename specifically to make this hold.
    #[cfg(unix)]
    #[test]
    fn test_save_to_sets_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");

        Settings::default().save_to(&path).unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        // Mask off the file-type bits — only the permission bits matter.
        assert_eq!(mode & 0o777, 0o600, "got {:o}", mode & 0o777);
    }

    #[test]
    fn test_load_from_corrupt_file_does_not_trigger_save() {
        // If the corrupt-file path accidentally called save_to(), the newly-created
        // defaults would be written back to `path` and we'd lose the guarantee that
        // the original is preserved. Assert that no fresh settings.json appears.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{ invalid").unwrap();

        let _ = Settings::load_from(&path);

        // No settings.json exists — it was moved to .corrupt-*, and load_from
        // did NOT write a new one. The next explicit save() will create it.
        assert!(!path.exists());
    }

    #[test]
    fn test_load_from_missing_field_uses_default() {
        // The single most common upgrade path: a new version adds a field.
        // Old files should load cleanly with the new field defaulted.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");

        // Hand-write a settings file that's missing `require_image_approval`.
        // Every other required field is present.
        let minimal = serde_json::json!({
            "output_dir": "/tmp/whatever",
            "blender_path": null,
            "provider_api_keys": { "fal-ai": "k" },
            "export_fbx_default": false,
        });
        std::fs::write(&path, serde_json::to_string(&minimal).unwrap()).unwrap();

        let loaded = Settings::load_from(&path);

        // Missing field falls back to Default::default() — not an error.
        assert_eq!(
            loaded.require_image_approval,
            Settings::default().require_image_approval
        );
        // Present fields survived.
        assert_eq!(
            loaded.provider_api_keys.get("fal-ai").map(String::as_str),
            Some("k")
        );
    }

    #[test]
    fn test_quarantine_path_contains_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        let q = quarantine_path(&p);
        assert_eq!(q.parent(), Some(dir.path()));

        let prefix = corrupt_prefix_for("settings.json");
        let name = q.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with(&prefix), "got {}", name);

        // Suffix is either digits (epoch seconds) or "unknown".
        let suffix = &name[prefix.len()..];
        assert!(
            suffix == "unknown" || suffix.chars().all(|c| c.is_ascii_digit()),
            "unexpected suffix: {}",
            suffix
        );
    }

    /// When the settings file doesn't exist and we can't create one (e.g.,
    /// the parent directory is read-only), `load_with_status` should report
    /// `InitialCreateFailed` instead of returning `Ok` with a phantom file.
    /// Without this, the user enters API keys, quits, and loses everything
    /// silently because no on-disk file ever existed.
    #[cfg(unix)]
    #[test]
    fn test_load_from_with_status_reports_create_failure() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        // Make the parent directory read-only so the create_dir_all + File::create
        // chain in save_to fails.
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o500)).unwrap();
        let path = dir.path().join("settings.json");

        let (_settings, status) = Settings::load_from_with_status(&path);

        // Restore perms so tempfile can clean up.
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();

        match status {
            LoadStatus::InitialCreateFailed { settings_path, .. } => {
                assert_eq!(settings_path, path);
            }
            other => panic!("expected InitialCreateFailed, got {:?}", other),
        }
    }

    #[test]
    fn test_quarantine_path_collision_adds_counter_suffix() {
        // Simulate two corrupt files within the same clock second. The second
        // call must not collide with or overwrite the first.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");

        let first = quarantine_path(&p);
        // Materialize the first quarantine file so the second call sees a
        // collision on the plain-timestamp path.
        std::fs::write(&first, b"first corrupt copy").unwrap();

        let second = quarantine_path(&p);
        assert_ne!(first, second, "collision should yield a distinct path");
        assert!(!second.exists(), "second path should be fresh");

        // The collision suffix should match the `-N` convention.
        let second_name = second.file_name().unwrap().to_string_lossy().into_owned();
        let first_name = first.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(second_name, format!("{}-1", first_name));
    }
}
