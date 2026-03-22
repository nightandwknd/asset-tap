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
    let dir_path = base_dir.join(generate_timestamp());
    std::fs::create_dir_all(&dir_path)?;
    Ok(dir_path)
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
