//! Configuration and utility functions.
//!
//! This module provides configuration helpers and directory utilities for
//! the Asset Tap, including output directory management and model lookups.
//!
//! # See Also
//!
//! - [`settings`](crate::settings) - User settings and persistence
//! - [`pipeline::PipelineConfig`](crate::pipeline::PipelineConfig) - Pipeline configuration

use crate::constants::files::dev_dirs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

// =============================================================================
// Directories
// =============================================================================

/// Base output directory for all generated assets.
///
/// **Note**: This is a fallback for development/CLI use. The GUI should use
/// `Settings::load().output_dir` for the user-configured path.
///
/// - Dev mode: `.dev/output/`
/// - Release mode: `output/`
pub static OUTPUT_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    if cfg!(debug_assertions) {
        PathBuf::from(dev_dirs::OUTPUT)
    } else {
        PathBuf::from("output")
    }
});

// =============================================================================
// Utility Functions
// =============================================================================

/// Create a new generation directory with timestamp.
pub fn create_generation_dir() -> Result<PathBuf, std::io::Error> {
    create_generation_dir_in(&OUTPUT_DIR)
}

/// Create a generation directory in a specific base path.
pub fn create_generation_dir_in(base_dir: &Path) -> Result<PathBuf, std::io::Error> {
    let dir_path = unique_timestamped_path(base_dir);
    std::fs::create_dir_all(&dir_path)?;
    Ok(dir_path)
}

/// Build a path under `base_dir` with the current timestamp as the directory
/// name, disambiguating with `-1`, `-2`, ... if a sibling with that name
/// already exists.
///
/// Two generations within the same wall-clock second produce the same
/// `generate_timestamp()` string. Without disambiguation, the second one
/// either silently merges into the first (when callers use `create_dir_all`)
/// or fails outright (when callers use `rename` to materialize the dir).
/// This helper is the single source of truth for "give me a fresh
/// timestamped output dir name."
///
/// The returned path does not exist at the moment of return, but the caller
/// is responsible for actually creating it. There's no protection against a
/// second process creating the same name in the gap — that's an exotic enough
/// failure mode that a plain check-then-create is fine for our use case.
pub fn unique_timestamped_path(base_dir: &Path) -> PathBuf {
    find_unused_with_counter_suffix(base_dir.join(generate_timestamp()))
}

/// Given a candidate path, return the first variant that doesn't already exist
/// on disk: try `base` first, then `base-1`, `base-2`, ..., up to a small cap.
///
/// Used by [`unique_timestamped_path`] (here) and `quarantine_path` (in
/// `settings.rs`) to disambiguate same-second collisions on filesystem
/// targets. The two callers build different bases — a timestamped output
/// directory in one case, a `<filename>.corrupt-<ts>` quarantine sibling in
/// the other — but the disambiguation logic is identical, so it lives here.
///
/// The retry cap (1000) is a safety net: 1000 collisions in a single second
/// indicates a much bigger problem than a check-then-create loop can usefully
/// recover from. If we run out, we return the original `base` and let the
/// caller's subsequent `create_dir_all` / `rename` either succeed (merging
/// or overwriting) or surface the real error.
pub fn find_unused_with_counter_suffix(base: PathBuf) -> PathBuf {
    if !base.exists() {
        return base;
    }
    // We need both the parent dir and the base filename to construct
    // siblings. If `base` has no parent or no filename it's already a
    // pathological input — return it unchanged and let the caller handle it.
    let (parent, stem) = match (base.parent(), base.file_name().and_then(|n| n.to_str())) {
        (Some(p), Some(s)) => (p, s),
        _ => return base,
    };
    for i in 1..1000 {
        let candidate = parent.join(format!("{stem}-{i}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    base
}

/// List available text-to-image models from a provider registry.
pub fn list_text_to_image_models(registry: &crate::providers::ProviderRegistry) -> Vec<String> {
    use crate::providers::ProviderCapability;

    registry
        .list_available()
        .iter()
        .flat_map(|provider| provider.list_models(ProviderCapability::TextToImage))
        .map(|model| model.id)
        .collect()
}

/// List available image-to-3D models from a provider registry.
pub fn list_image_to_3d_models(registry: &crate::providers::ProviderRegistry) -> Vec<String> {
    use crate::providers::ProviderCapability;

    registry
        .list_available()
        .iter()
        .flat_map(|provider| provider.list_models(ProviderCapability::ImageTo3D))
        .map(|model| model.id)
        .collect()
}

/// Get the default text-to-image model from the first available provider.
pub fn get_default_text_to_image_model(
    registry: &crate::providers::ProviderRegistry,
) -> Option<String> {
    use crate::providers::ProviderCapability;

    registry.get_default().and_then(|provider| {
        provider
            .list_models(ProviderCapability::TextToImage)
            .into_iter()
            .find(|m| m.is_default)
            .map(|m| m.id)
    })
}

/// Get the default image-to-3D model from the first available provider.
pub fn get_default_image_to_3d_model(
    registry: &crate::providers::ProviderRegistry,
) -> Option<String> {
    use crate::providers::ProviderCapability;

    registry.get_default().and_then(|provider| {
        provider
            .list_models(ProviderCapability::ImageTo3D)
            .into_iter()
            .find(|m| m.is_default)
            .map(|m| m.id)
    })
}

/// Generate a timestamp string for unique IDs.
///
/// Format: `YYYY-MM-DD_HHMMSS` (e.g., `2026-02-22_111547`)
/// Matches the ISO 8601 date format used by tracing-appender log files.
pub fn generate_timestamp() -> String {
    use chrono::Local;
    Local::now().format("%Y-%m-%d_%H%M%S").to_string()
}
