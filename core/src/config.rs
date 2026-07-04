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

// =============================================================================
// Atomic file writes
// =============================================================================

/// Options controlling [`atomic_write`]. The default (no backup, no permission
/// clamp) suits caches and app state; settings opt into both.
#[derive(Clone, Copy, Debug, Default)]
pub struct AtomicWriteOptions {
    /// Copy the existing file to `<path>.bak` before replacing it.
    pub backup: bool,
    /// On Unix, set the final file's mode to `0o600` (owner-only). Use for
    /// files that may contain secrets (e.g. API keys).
    pub owner_only: bool,
}

/// Durably and atomically write `contents` to `path`.
///
/// This is the shared implementation behind every persisted JSON file in the
/// app. It writes to a sibling temp file, `fsync`s it, optionally clamps
/// permissions and backs up the previous file, then `rename`s into place — so a
/// crash mid-write can never leave a truncated or empty file where a valid one
/// used to be. The rename is atomic because the temp file lives in the same
/// directory (hence the same filesystem) as the destination.
///
/// Prefer this over a bare `std::fs::write` for anything whose corruption would
/// lose user data (settings, app state, history, bundle metadata, caches).
pub fn atomic_write(path: &Path, contents: &[u8], opts: AtomicWriteOptions) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Unique temp sibling so concurrent writers to different files don't clash;
    // `.tmp` extension keeps it recognizable if a crash leaves one behind.
    let tmp_path = path.with_extension(match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => format!("{ext}.tmp"),
        None => "tmp".to_string(),
    });

    {
        use std::io::Write as _;
        let mut tmp = std::fs::File::create(&tmp_path)?;
        tmp.write_all(contents)?;
        // fsync before rename: without it, a crash between write() and rename()
        // can promote an empty tmp file to the real file.
        tmp.sync_all()?;
    }

    #[cfg(unix)]
    if opts.owner_only {
        use std::os::unix::fs::PermissionsExt;
        // Clamp permissions on the tmp file *before* the rename so the final
        // file is never world-readable, even momentarily.
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o600))?;
    }

    if opts.backup && path.exists() {
        let bak_path = path.with_extension(match path.extension().and_then(|e| e.to_str()) {
            Some(ext) => format!("{ext}.bak"),
            None => "bak".to_string(),
        });
        if let Err(e) = std::fs::copy(path, &bak_path) {
            // Non-fatal: saving the new data matters more than the backup.
            tracing::warn!("Failed to back up {:?} to {:?}: {}", path, bak_path, e);
        }
    }

    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

/// Serialize `value` as pretty JSON and [`atomic_write`] it to `path`.
pub fn atomic_write_json<T: serde::Serialize>(
    path: &Path,
    value: &T,
    opts: AtomicWriteOptions,
) -> std::io::Result<()> {
    let contents = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    atomic_write(path, &contents, opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atomic_write_roundtrip_and_no_tmp_left() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("data.json");
        atomic_write(&path, b"hello", AtomicWriteOptions::default()).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"hello");
        // No stray tmp sibling remains after a successful write.
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn test_atomic_write_backs_up_previous() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("data.json");
        let opts = AtomicWriteOptions {
            backup: true,
            owner_only: false,
        };
        atomic_write(&path, b"v1", opts).unwrap();
        atomic_write(&path, b"v2", opts).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"v2");
        assert_eq!(
            std::fs::read(path.with_extension("json.bak")).unwrap(),
            b"v1"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_atomic_write_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("secret.json");
        atomic_write(
            &path,
            b"key",
            AtomicWriteOptions {
                backup: false,
                owner_only: true,
            },
        )
        .unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
