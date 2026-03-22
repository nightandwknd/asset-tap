//! Bundle metadata management.
//!
//! Each pipeline generation creates an output **bundle** - a directory containing
//! the generated assets and a `bundle.json` metadata file.
//!
//! # Bundle Structure
//!
//! ```text
//! output/20240115_143022/
//! ├── bundle.json      # Metadata (this module)
//! ├── image.png        # Generated image
//! ├── model.glb        # 3D model
//! ├── model.fbx        # FBX export (optional)
//! └── textures/        # Extracted textures
//! ```
//!
//! # Metadata Contents
//!
//! The `bundle.json` file contains:
//! - Custom display name
//! - Creation timestamp
//! - Generation configuration (prompt, models used)
//! - Model statistics (vertex count, file size)
//! - User tags and favorites
//! - Generation duration
//!
//! This module handles loading, saving, and error-tolerant discovery of bundles.
//!
//! # See Also
//!
//! - [`PipelineOutput`](crate::types::PipelineOutput) - Pipeline execution results
//! - [`history`](crate::history) - Generation history tracking

use crate::constants::files::bundle as bundle_files;
use crate::constants::validation;
use crate::history::GenerationConfig;
use crate::state::ModelInfo;
use chrono::{DateTime, Utc};
use include_dir::{include_dir, Dir};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// Embedded default bundles (all subdirectories from bundles/).
static EMBEDDED_BUNDLES: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../bundles");

/// Metadata filename within each bundle directory.
const BUNDLE_METADATA_FILE: &str = bundle_files::METADATA;

/// Re-export standard file names for convenience.
pub mod files {
    pub use crate::constants::files::bundle::*;
}

/// Metadata stored in bundle.json within each generation directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BundleMetadata {
    /// Schema version for forward compatibility.
    pub version: u32,

    /// Custom display name (None = use directory name).
    pub name: Option<String>,

    /// When the bundle was created.
    pub created_at: DateTime<Utc>,

    /// Generation configuration used (prompt, models, etc.).
    pub config: Option<GenerationConfig>,

    /// Model statistics (vertex count, file size, etc.).
    /// Populated lazily on first view.
    pub model_info: Option<ModelInfo>,

    /// Duration of generation in milliseconds.
    pub duration_ms: Option<u64>,

    /// User-defined tags for organization.
    pub tags: Vec<String>,

    /// Marked as favorite for quick access.
    pub favorite: bool,

    /// Notes or description added by user.
    pub notes: Option<String>,
}

impl Default for BundleMetadata {
    fn default() -> Self {
        Self {
            version: 1,
            name: None,
            created_at: Utc::now(),
            config: None,
            model_info: None,
            duration_ms: None,
            tags: Vec::new(),
            favorite: false,
            notes: None,
        }
    }
}

impl BundleMetadata {
    /// Create new metadata with creation timestamp.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create metadata with generation config.
    pub fn with_config(config: GenerationConfig) -> Self {
        Self {
            config: Some(config),
            ..Default::default()
        }
    }

    /// Load metadata from a bundle directory.
    ///
    /// Returns `Ok(None)` if the file doesn't exist.
    /// Returns `Err` only for I/O errors that aren't "not found".
    pub fn load(bundle_dir: &Path) -> Result<Option<Self>, BundleError> {
        let path = bundle_dir.join(BUNDLE_METADATA_FILE);

        if !path.exists() {
            return Ok(None);
        }

        let contents = std::fs::read_to_string(&path).map_err(|e| BundleError::Io {
            path: path.clone(),
            source: e,
        })?;

        match serde_json::from_str::<Self>(&contents) {
            Ok(mut metadata) => {
                // Validate and sanitize the loaded metadata
                let issues = metadata.validate_and_sanitize();
                if !issues.is_empty() {
                    warn!(
                        "Fixed {} validation issue(s) in bundle.json at {}:",
                        issues.len(),
                        path.display()
                    );
                    for issue in &issues {
                        warn!("  - {}", issue);
                    }
                    // Automatically save the cleaned version
                    if let Err(e) = metadata.save(bundle_dir) {
                        warn!("Failed to save sanitized metadata: {}", e);
                    } else {
                        debug!("Saved sanitized metadata to {}", path.display());
                    }
                }
                Ok(Some(metadata))
            }
            Err(e) => {
                warn!(
                    "Invalid bundle.json at {}: {}. Returning error.",
                    path.display(),
                    e
                );
                Err(BundleError::InvalidJson { path, source: e })
            }
        }
    }

    /// Save metadata to a bundle directory.
    pub fn save(&self, bundle_dir: &Path) -> Result<(), BundleError> {
        let path = bundle_dir.join(BUNDLE_METADATA_FILE);

        let contents = serde_json::to_string_pretty(self)
            .map_err(|e| BundleError::Serialization { source: e })?;

        std::fs::write(&path, contents).map_err(|e| BundleError::Io { path, source: e })
    }

    /// Validate and sanitize this metadata, fixing any corrupt or out-of-bounds values.
    ///
    /// This is called automatically when loading from disk. It will:
    /// - Clamp numeric values to reasonable ranges
    /// - Truncate overly long strings
    /// - Remove invalid tags
    /// - Fix timestamps that are too far in the future
    ///
    /// Returns a list of issues that were fixed.
    pub fn validate_and_sanitize(&mut self) -> Vec<String> {
        let mut issues = Vec::new();

        // Validate version
        if self.version > validation::MAX_VERSION {
            issues.push(format!(
                "Schema version {} exceeds maximum {}, clamping",
                self.version,
                validation::MAX_VERSION
            ));
            self.version = validation::MAX_VERSION;
        }

        // Sanitize name
        if let Some(ref mut name) = self.name {
            let original_len = name.len();
            *name = sanitize_string(name, validation::MAX_NAME_LENGTH);
            if name.len() != original_len {
                issues.push(format!(
                    "Name truncated from {} to {} characters",
                    original_len,
                    name.len()
                ));
            }
            // Clear if empty after sanitization
            if name.is_empty() {
                self.name = None;
            }
        }

        // Sanitize notes
        if let Some(ref mut notes) = self.notes {
            let original_len = notes.len();
            *notes = sanitize_string(notes, validation::MAX_NOTES_LENGTH);
            if notes.len() != original_len {
                issues.push(format!(
                    "Notes truncated from {} to {} characters",
                    original_len,
                    notes.len()
                ));
            }
            if notes.is_empty() {
                self.notes = None;
            }
        }

        // Validate and sanitize tags
        let original_tag_count = self.tags.len();
        self.tags.retain(|tag| !tag.trim().is_empty());
        self.tags = self
            .tags
            .iter()
            .map(|tag| sanitize_string(tag, validation::MAX_TAG_LENGTH))
            .collect();
        self.tags.dedup();
        self.tags.truncate(validation::MAX_TAGS);

        if self.tags.len() != original_tag_count {
            issues.push(format!(
                "Tags reduced from {} to {} (removed duplicates/invalid)",
                original_tag_count,
                self.tags.len()
            ));
        }

        // Validate duration
        if let Some(duration) = self.duration_ms {
            if duration > validation::MAX_DURATION_MS {
                issues.push(format!(
                    "Duration {} ms exceeds maximum, clamping",
                    duration
                ));
                self.duration_ms = Some(validation::MAX_DURATION_MS);
            }
        }

        // Validate timestamp (not too far in the future)
        let now = Utc::now();
        let max_future = now + chrono::Duration::seconds(validation::FUTURE_TOLERANCE_SECS);
        if self.created_at > max_future {
            issues.push(format!(
                "Timestamp {} is too far in the future, resetting to now",
                self.created_at
            ));
            self.created_at = now;
        }

        issues
    }

    /// Get the display name for this bundle.
    ///
    /// Returns custom name if set, otherwise returns None (caller should use dir name).
    pub fn display_name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Set a custom name for this bundle.
    pub fn set_name(&mut self, name: impl Into<String>) {
        let name = sanitize_string(&name.into(), validation::MAX_NAME_LENGTH);
        self.name = if name.trim().is_empty() {
            None
        } else {
            Some(name)
        };
    }

    /// Clear the custom name (revert to directory name display).
    pub fn clear_name(&mut self) {
        self.name = None;
    }

    /// Add a tag to this bundle.
    pub fn add_tag(&mut self, tag: impl Into<String>) {
        let tag = sanitize_string(&tag.into(), validation::MAX_TAG_LENGTH);
        if !tag.trim().is_empty()
            && !self.tags.contains(&tag)
            && self.tags.len() < validation::MAX_TAGS
        {
            self.tags.push(tag);
        }
    }

    /// Remove a tag from this bundle.
    pub fn remove_tag(&mut self, tag: &str) {
        self.tags.retain(|t| t != tag);
    }

    /// Toggle favorite status.
    pub fn toggle_favorite(&mut self) {
        self.favorite = !self.favorite;
    }
}

/// A discovered bundle with its path and metadata.
#[derive(Debug, Clone)]
pub struct Bundle {
    /// Path to the bundle directory.
    pub path: PathBuf,

    /// Bundle metadata (loaded or inferred).
    pub metadata: BundleMetadata,

    /// What files exist in this bundle.
    pub contents: BundleContents,

    /// Any issues detected with this bundle.
    pub issues: Vec<BundleIssue>,
}

impl Bundle {
    /// Get the display name for this bundle.
    ///
    /// Priority: custom name > directory name
    pub fn display_name(&self) -> &str {
        self.metadata
            .display_name()
            .unwrap_or_else(|| self.dir_name())
    }

    /// Get the directory name.
    pub fn dir_name(&self) -> &str {
        self.path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
    }

    /// Check if this appears to be a timestamp-named directory.
    pub fn has_timestamp_name(&self) -> bool {
        let name = self.dir_name();
        // Format: YYYY-MM-DD_HHMMSS (17 chars)
        name.len() == 17 && name.chars().nth(10) == Some('_')
    }

    /// Rename this bundle (updates metadata, not directory).
    pub fn rename(&mut self, new_name: impl Into<String>) -> Result<(), BundleError> {
        self.metadata.set_name(new_name);
        self.metadata.save(&self.path)
    }

    /// Save any metadata changes.
    pub fn save(&self) -> Result<(), BundleError> {
        self.metadata.save(&self.path)
    }

    /// Check if this bundle has any issues.
    pub fn has_issues(&self) -> bool {
        !self.issues.is_empty()
    }

    /// Check if this bundle is missing metadata.
    pub fn needs_migration(&self) -> bool {
        self.issues
            .iter()
            .any(|i| matches!(i, BundleIssue::MissingMetadata))
    }
}

impl From<Bundle> for crate::types::PipelineOutput {
    fn from(bundle: Bundle) -> Self {
        Self {
            output_dir: Some(bundle.path),
            image_path: bundle.contents.image,
            model_path: bundle.contents.model,
            fbx_path: bundle.contents.model_fbx,
            textures_dir: bundle.contents.textures_dir,
            ..Default::default()
        }
    }
}

/// What files exist within a bundle.
#[derive(Debug, Clone, Default)]
pub struct BundleContents {
    /// Image file path (if exists).
    pub image: Option<PathBuf>,

    /// Main model file path (if exists).
    pub model: Option<PathBuf>,

    /// FBX export file path (if exists).
    pub model_fbx: Option<PathBuf>,

    /// Textures directory path (if exists and has files).
    pub textures_dir: Option<PathBuf>,

    /// Number of texture files found.
    pub texture_count: usize,
}

impl BundleContents {
    /// Check if this bundle has any viewable content.
    pub fn has_content(&self) -> bool {
        self.image.is_some() || self.model.is_some()
    }

    /// Check if this bundle has a 3D model.
    pub fn has_model(&self) -> bool {
        self.model.is_some()
    }
}

/// Issues detected with a bundle (non-fatal).
#[derive(Debug, Clone)]
pub enum BundleIssue {
    /// bundle.json is missing (metadata was inferred from directory).
    MissingMetadata,

    /// bundle.json exists but couldn't be parsed.
    InvalidMetadata(String),

    /// Expected file is missing.
    MissingFile(String),

    /// File exists but may be corrupted or zero-size.
    SuspiciousFile { file: String, reason: String },

    /// Directory structure is unusual.
    UnexpectedStructure(String),
}

/// Errors that can occur during bundle operations.
#[derive(Debug, thiserror::Error)]
pub enum BundleError {
    /// I/O error reading or writing files.
    #[error("I/O error at {}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    /// JSON parsing error.
    #[error("Invalid JSON at {}: {source}", path.display())]
    InvalidJson {
        path: PathBuf,
        source: serde_json::Error,
    },

    /// Serialization error.
    #[error("Serialization error: {source}")]
    Serialization { source: serde_json::Error },

    /// Bundle directory doesn't exist.
    #[error("Bundle not found: {}", .0.display())]
    NotFound(PathBuf),

    /// Not a valid bundle directory.
    #[error("Not a valid bundle: {}", .0.display())]
    NotABundle(PathBuf),
}

/// Ensure default demo bundles exist in the output directory.
///
/// On first run, copies all embedded demo bundles to the output directory so new
/// users have a showcase asset in their library. Each embedded bundle directory
/// (e.g., `bundles/asset-tap/`) is copied as a timestamped subdirectory.
///
/// Bundles are only written if they don't already exist (checked by a marker
/// directory name). This is idempotent and safe to call on every startup.
pub fn ensure_default_bundles_exist(output_dir: &Path) {
    // Create output directory if it doesn't exist
    if let Err(e) = std::fs::create_dir_all(output_dir) {
        warn!("Failed to create output directory: {}", e);
        return;
    }

    // Iterate through embedded bundle directories
    for bundle_dir in EMBEDDED_BUNDLES.dirs() {
        let bundle_name = match bundle_dir.path().file_name().and_then(|n| n.to_str()) {
            Some(name) => name,
            None => continue,
        };

        // Use a fixed directory name based on the bundle name so we can detect duplicates.
        // Prefix with underscore to distinguish from user-generated timestamped dirs.
        let target_dir_name = format!("_{}", bundle_name);
        let target_dir = output_dir.join(&target_dir_name);

        // Skip if already seeded
        if target_dir.exists() {
            debug!("Demo bundle '{}' already exists, skipping", bundle_name);
            continue;
        }

        // Create the target directory
        if let Err(e) = std::fs::create_dir_all(&target_dir) {
            warn!("Failed to create demo bundle directory: {}", e);
            continue;
        }

        // Copy all files from the embedded bundle
        let mut files_written = 0;
        for file in bundle_dir.files() {
            let file_name = match file.path().file_name().and_then(|n| n.to_str()) {
                Some(name) => name,
                None => continue,
            };

            let target_path = target_dir.join(file_name);
            if let Err(e) = std::fs::write(&target_path, file.contents()) {
                warn!("Failed to write demo bundle file {:?}: {}", file_name, e);
            } else {
                files_written += 1;
            }
        }

        if files_written > 0 {
            info!(
                "Seeded demo bundle '{}' ({} files) to {}",
                bundle_name,
                files_written,
                target_dir.display()
            );
        }
    }
}

/// Discover all bundles in an output directory.
///
/// This function is error-tolerant: it will skip directories that don't look
/// like bundles and log warnings for any issues encountered.
pub fn discover_bundles(output_dir: &Path) -> Vec<Bundle> {
    let mut bundles = Vec::new();

    let entries = match std::fs::read_dir(output_dir) {
        Ok(entries) => entries,
        Err(e) => {
            warn!(
                "Failed to read output directory {}: {}",
                output_dir.display(),
                e
            );
            return bundles;
        }
    };

    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();

        // Skip non-directories
        if !path.is_dir() {
            continue;
        }

        // Skip hidden directories
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with('.'))
            .unwrap_or(true)
        {
            continue;
        }

        match load_bundle(&path) {
            Ok(bundle) => {
                debug!("Discovered bundle: {}", bundle.display_name());
                bundles.push(bundle);
            }
            Err(BundleError::NotABundle(_)) => {
                // Not a bundle, skip silently
                debug!("Skipping non-bundle directory: {}", path.display());
            }
            Err(e) => {
                warn!("Error loading bundle at {}: {}", path.display(), e);
            }
        }
    }

    // Sort by creation date (newest first), falling back to directory name
    bundles.sort_by(|a, b| {
        b.metadata
            .created_at
            .cmp(&a.metadata.created_at)
            .then_with(|| a.dir_name().cmp(b.dir_name()))
    });

    bundles
}

/// Load a single bundle from a directory.
///
/// Returns `NotABundle` if the directory doesn't contain any recognized assets.
pub fn load_bundle(bundle_dir: &Path) -> Result<Bundle, BundleError> {
    if !bundle_dir.exists() {
        return Err(BundleError::NotFound(bundle_dir.to_path_buf()));
    }

    if !bundle_dir.is_dir() {
        return Err(BundleError::NotABundle(bundle_dir.to_path_buf()));
    }

    let mut issues = Vec::new();
    let mut contents = BundleContents::default();

    // Scan for known files
    scan_bundle_contents(bundle_dir, &mut contents, &mut issues);

    // If no recognized content, it's not a bundle
    if !contents.has_content() {
        return Err(BundleError::NotABundle(bundle_dir.to_path_buf()));
    }

    // Load or create metadata
    let metadata = match BundleMetadata::load(bundle_dir) {
        Ok(Some(meta)) => meta,
        Ok(None) => {
            // No bundle.json - infer metadata from directory name
            issues.push(BundleIssue::MissingMetadata);
            debug!(
                "No bundle.json at {}, inferring metadata from directory name",
                bundle_dir.display()
            );
            infer_metadata_from_dir(bundle_dir)
        }
        Err(BundleError::InvalidJson { path, source }) => {
            issues.push(BundleIssue::InvalidMetadata(source.to_string()));
            warn!("Invalid metadata at {}, using inferred", path.display());
            infer_metadata_from_dir(bundle_dir)
        }
        Err(e) => return Err(e),
    };

    Ok(Bundle {
        path: bundle_dir.to_path_buf(),
        metadata,
        contents,
        issues,
    })
}

/// Scan a bundle directory for known files.
fn scan_bundle_contents(
    bundle_dir: &Path,
    contents: &mut BundleContents,
    issues: &mut Vec<BundleIssue>,
) {
    // Check for image
    let image_path = bundle_dir.join(files::IMAGE);
    if image_path.exists() {
        if is_valid_file(&image_path) {
            contents.image = Some(image_path);
        } else {
            issues.push(BundleIssue::SuspiciousFile {
                file: files::IMAGE.to_string(),
                reason: "File is empty or inaccessible".to_string(),
            });
        }
    }

    // Also check for other image extensions
    for ext in &["jpg", "jpeg", "webp"] {
        let alt_path = bundle_dir.join(format!("image.{}", ext));
        if alt_path.exists() && contents.image.is_none() && is_valid_file(&alt_path) {
            contents.image = Some(alt_path);
        }
    }

    // Check for model.glb (standard filename)
    let model_path = bundle_dir.join(files::MODEL_GLB);
    if model_path.exists() {
        if is_valid_file(&model_path) {
            contents.model = Some(model_path);
        } else {
            issues.push(BundleIssue::SuspiciousFile {
                file: files::MODEL_GLB.to_string(),
                reason: "File is empty or inaccessible".to_string(),
            });
        }
    }

    // Check for model.fbx (standard filename)
    let fbx_path = bundle_dir.join(files::MODEL_FBX);
    if fbx_path.exists() && is_valid_file(&fbx_path) {
        contents.model_fbx = Some(fbx_path);
    }

    // Check for textures directory
    let textures_dir = bundle_dir.join(files::TEXTURES_DIR);
    if textures_dir.exists() && textures_dir.is_dir() {
        let texture_count = count_textures(&textures_dir);
        if texture_count > 0 {
            contents.textures_dir = Some(textures_dir);
            contents.texture_count = texture_count;
        }
    }
}

/// Check if a file exists and has non-zero size.
fn is_valid_file(path: &Path) -> bool {
    path.metadata()
        .map(|m| m.is_file() && m.len() > 0)
        .unwrap_or(false)
}

/// Count texture files in a directory.
fn count_textures(textures_dir: &Path) -> usize {
    std::fs::read_dir(textures_dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|e| {
                    e.path()
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .map(|ext| matches!(ext.to_lowercase().as_str(), "png" | "jpg" | "jpeg"))
                        .unwrap_or(false)
                })
                .count()
        })
        .unwrap_or(0)
}

/// Sanitize a string by removing control characters and limiting length.
///
/// This protects against:
/// - Control characters that could mess up terminal/UI rendering
/// - Extremely long strings that could cause DoS
/// - Null bytes and other binary data
fn sanitize_string(input: &str, max_len: usize) -> String {
    input
        .chars()
        .filter(|c| {
            // Allow printable characters, spaces, and common whitespace
            !c.is_control() || matches!(c, '\n' | '\r' | '\t')
        })
        .take(max_len)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Infer metadata from directory name when bundle.json is missing.
fn infer_metadata_from_dir(bundle_dir: &Path) -> BundleMetadata {
    let mut metadata = BundleMetadata::default();

    // Try to parse timestamp from directory name
    if let Some(name) = bundle_dir.file_name().and_then(|n| n.to_str()) {
        if let Some(dt) = parse_timestamp_dir_name(name) {
            metadata.created_at = dt;
        }
    }

    metadata
}

/// Parse a timestamp directory name (`YYYY-MM-DD_HHMMSS`) into a DateTime.
fn parse_timestamp_dir_name(name: &str) -> Option<DateTime<Utc>> {
    // Format: YYYY-MM-DD_HHMMSS (17 chars)
    if name.len() != 17 || name.chars().nth(10) != Some('_') {
        return None;
    }

    let year: i32 = name[0..4].parse().ok()?;
    let month: u32 = name[5..7].parse().ok()?;
    let day: u32 = name[8..10].parse().ok()?;
    let time_part = &name[11..17];

    let hour: u32 = time_part[0..2].parse().ok()?;
    let minute: u32 = time_part[2..4].parse().ok()?;
    let second: u32 = time_part[4..6].parse().ok()?;

    chrono::NaiveDate::from_ymd_opt(year, month, day)
        .and_then(|d| d.and_hms_opt(hour, minute, second))
        .map(|dt| dt.and_utc())
}

/// Export a bundle directory as a zip archive.
///
/// Recursively adds all files in the bundle directory to the archive.
/// Returns the number of files added.
pub fn export_bundle_zip(bundle_dir: &Path, dest: &Path) -> Result<usize, String> {
    let file = std::fs::File::create(dest).map_err(|e| format!("Failed to create zip: {}", e))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let mut count = 0;
    add_dir_to_zip(&mut zip, bundle_dir, bundle_dir, options, &mut count)?;
    zip.finish()
        .map_err(|e| format!("Failed to finalize zip: {}", e))?;
    Ok(count)
}

/// Recursively add a directory's contents to a zip archive.
fn add_dir_to_zip(
    zip: &mut zip::ZipWriter<std::fs::File>,
    dir: &Path,
    base: &Path,
    options: zip::write::SimpleFileOptions,
    count: &mut usize,
) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("Failed to read directory: {}", e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        let relative = path
            .strip_prefix(base)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();

        if path.is_dir() {
            add_dir_to_zip(zip, &path, base, options, count)?;
        } else {
            let data =
                std::fs::read(&path).map_err(|e| format!("Failed to read {}: {}", relative, e))?;
            zip.start_file(&relative, options)
                .map_err(|e| format!("Failed to add {}: {}", relative, e))?;
            use std::io::Write;
            zip.write_all(&data)
                .map_err(|e| format!("Failed to write {}: {}", relative, e))?;
            *count += 1;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Timelike};

    #[test]
    fn test_parse_timestamp_dir_name() {
        // New format
        let dt = parse_timestamp_dir_name("2025-12-25_200109").unwrap();
        assert_eq!(dt.year(), 2025);
        assert_eq!(dt.month(), 12);
        assert_eq!(dt.day(), 25);
        assert_eq!(dt.hour(), 20);
        assert_eq!(dt.minute(), 1);
        assert_eq!(dt.second(), 9);
    }

    #[test]
    fn test_parse_timestamp_invalid() {
        assert!(parse_timestamp_dir_name("not_a_timestamp").is_none());
        assert!(parse_timestamp_dir_name("20251225_200109").is_none()); // Old format not supported
        assert!(parse_timestamp_dir_name("20251225200109").is_none()); // No underscore
        assert!(parse_timestamp_dir_name("2025-12-25_20:01:09").is_none()); // Colons in time
    }

    #[test]
    fn test_bundle_metadata_default() {
        let meta = BundleMetadata::default();
        assert!(meta.name.is_none());
        assert!(meta.tags.is_empty());
        assert!(!meta.favorite);
        assert_eq!(meta.version, 1);
    }

    #[test]
    fn test_bundle_metadata_set_name() {
        let mut meta = BundleMetadata::default();

        meta.set_name("My Cool Model");
        assert_eq!(meta.name, Some("My Cool Model".to_string()));

        meta.set_name("   ");
        assert!(meta.name.is_none());

        meta.set_name("");
        assert!(meta.name.is_none());
    }

    #[test]
    fn test_bundle_metadata_tags() {
        let mut meta = BundleMetadata::default();

        meta.add_tag("robot");
        meta.add_tag("sci-fi");
        assert_eq!(meta.tags.len(), 2);

        // Don't add duplicates
        meta.add_tag("robot");
        assert_eq!(meta.tags.len(), 2);

        meta.remove_tag("robot");
        assert_eq!(meta.tags.len(), 1);
        assert_eq!(meta.tags[0], "sci-fi");
    }

    #[test]
    fn test_bundle_contents_has_content() {
        let empty = BundleContents::default();
        assert!(!empty.has_content());

        let with_image = BundleContents {
            image: Some(PathBuf::from(bundle_files::IMAGE)),
            ..Default::default()
        };
        assert!(with_image.has_content());

        let with_model = BundleContents {
            model: Some(PathBuf::from(bundle_files::MODEL_GLB)),
            ..Default::default()
        };
        assert!(with_model.has_content());
        assert!(with_model.has_model());
    }

    #[test]
    fn test_bundle_display_name() {
        let bundle = Bundle {
            path: PathBuf::from("/output/2025-12-25_200109"),
            metadata: BundleMetadata::default(),
            contents: BundleContents::default(),
            issues: vec![],
        };
        assert_eq!(bundle.display_name(), "2025-12-25_200109");

        let bundle_with_name = Bundle {
            path: PathBuf::from("/output/2025-12-25_200109"),
            metadata: BundleMetadata {
                name: Some("Cool Robot".to_string()),
                ..Default::default()
            },
            contents: BundleContents::default(),
            issues: vec![],
        };
        assert_eq!(bundle_with_name.display_name(), "Cool Robot");
    }

    #[test]
    fn test_has_timestamp_name() {
        let bundle = Bundle {
            path: PathBuf::from("/output/2025-12-25_200109"),
            metadata: BundleMetadata::default(),
            contents: BundleContents::default(),
            issues: vec![],
        };
        assert!(bundle.has_timestamp_name());

        let custom_bundle = Bundle {
            path: PathBuf::from("/output/my_cool_model"),
            metadata: BundleMetadata::default(),
            contents: BundleContents::default(),
            issues: vec![],
        };
        assert!(!custom_bundle.has_timestamp_name());
    }

    // =============================================================================
    // Validation and Sanitization Tests
    // =============================================================================

    #[test]
    fn test_sanitize_string() {
        // Normal strings are preserved
        assert_eq!(sanitize_string("Hello World", 100), "Hello World");

        // Leading/trailing whitespace is trimmed
        assert_eq!(sanitize_string("  Hello  ", 100), "Hello");

        // Control characters are removed (except newlines, tabs)
        assert_eq!(sanitize_string("Hello\x00World\x01", 100), "HelloWorld");

        // Newlines and tabs are preserved
        assert_eq!(sanitize_string("Hello\nWorld\t", 100), "Hello\nWorld");

        // Strings are truncated to max length
        assert_eq!(sanitize_string("Hello World", 5), "Hello");

        // Empty after sanitization
        assert_eq!(sanitize_string("\x00\x01\x02", 100), "");
    }

    #[test]
    fn test_validate_version_clamp() {
        let mut meta = BundleMetadata {
            version: 999,
            ..Default::default()
        };

        let issues = meta.validate_and_sanitize();
        assert_eq!(meta.version, validation::MAX_VERSION);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("version"));
    }

    #[test]
    fn test_validate_name_truncation() {
        let long_name = "A".repeat(1000);
        let mut meta = BundleMetadata {
            name: Some(long_name),
            ..Default::default()
        };

        let issues = meta.validate_and_sanitize();
        assert!(meta.name.is_some());
        assert_eq!(
            meta.name.as_ref().unwrap().len(),
            validation::MAX_NAME_LENGTH
        );
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("Name truncated"));
    }

    #[test]
    fn test_validate_name_control_chars() {
        let mut meta = BundleMetadata {
            name: Some("Hello\x00\x01World".to_string()),
            ..Default::default()
        };

        meta.validate_and_sanitize();
        assert_eq!(meta.name, Some("HelloWorld".to_string()));
    }

    #[test]
    fn test_validate_notes_truncation() {
        let long_notes = "B".repeat(20000);
        let mut meta = BundleMetadata {
            notes: Some(long_notes),
            ..Default::default()
        };

        let issues = meta.validate_and_sanitize();
        assert!(meta.notes.is_some());
        assert_eq!(
            meta.notes.as_ref().unwrap().len(),
            validation::MAX_NOTES_LENGTH
        );
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("Notes truncated"));
    }

    #[test]
    fn test_validate_tags_limit() {
        let mut tags = Vec::new();
        for i in 0..200 {
            tags.push(format!("tag{}", i));
        }

        let mut meta = BundleMetadata {
            tags,
            ..Default::default()
        };

        let issues = meta.validate_and_sanitize();
        assert_eq!(meta.tags.len(), validation::MAX_TAGS);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("Tags reduced"));
    }

    #[test]
    fn test_validate_tags_duplicates() {
        let mut meta = BundleMetadata {
            tags: vec!["robot".into(), "robot".into(), "sci-fi".into()],
            ..Default::default()
        };

        let issues = meta.validate_and_sanitize();
        assert_eq!(meta.tags.len(), 2);
        assert!(meta.tags.contains(&"robot".to_string()));
        assert!(meta.tags.contains(&"sci-fi".to_string()));
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn test_validate_tags_empty() {
        let mut meta = BundleMetadata {
            tags: vec!["".into(), "   ".into(), "valid".into()],
            ..Default::default()
        };

        let issues = meta.validate_and_sanitize();
        assert_eq!(meta.tags.len(), 1);
        assert_eq!(meta.tags[0], "valid");
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn test_validate_duration_clamp() {
        let mut meta = BundleMetadata {
            duration_ms: Some(999999999999),
            ..Default::default()
        };

        let issues = meta.validate_and_sanitize();
        assert_eq!(meta.duration_ms, Some(validation::MAX_DURATION_MS));
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("Duration"));
    }

    #[test]
    fn test_validate_timestamp_future() {
        let far_future = Utc::now() + chrono::Duration::days(365);
        let mut meta = BundleMetadata {
            created_at: far_future,
            ..Default::default()
        };

        let issues = meta.validate_and_sanitize();
        assert!(meta.created_at <= Utc::now() + chrono::Duration::seconds(5));
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("future"));
    }

    #[test]
    fn test_validate_timestamp_past() {
        let past = Utc::now() - chrono::Duration::days(365);
        let mut meta = BundleMetadata {
            created_at: past,
            ..Default::default()
        };

        let issues = meta.validate_and_sanitize();
        // Past timestamps are fine
        assert_eq!(meta.created_at, past);
        assert_eq!(issues.len(), 0);
    }

    #[test]
    fn test_validate_multiple_issues() {
        let mut tags = Vec::new();
        for i in 0..200 {
            tags.push(format!("tag{}", i));
        }

        let mut meta = BundleMetadata {
            version: 999,
            name: Some("A".repeat(1000)),
            tags,
            duration_ms: Some(999999999999),
            created_at: Utc::now() + chrono::Duration::days(365),
            ..Default::default()
        };

        let issues = meta.validate_and_sanitize();
        // Should fix all issues
        assert!(issues.len() >= 4);
        assert_eq!(meta.version, validation::MAX_VERSION);
        assert!(meta.name.as_ref().unwrap().len() <= validation::MAX_NAME_LENGTH);
        assert_eq!(meta.tags.len(), validation::MAX_TAGS);
        assert_eq!(meta.duration_ms, Some(validation::MAX_DURATION_MS));
        assert!(meta.created_at <= Utc::now() + chrono::Duration::seconds(5));
    }

    #[test]
    fn test_set_name_sanitizes() {
        let mut meta = BundleMetadata::default();

        // Normal name
        meta.set_name("My Model");
        assert_eq!(meta.name, Some("My Model".to_string()));

        // Name with control chars
        meta.set_name("Bad\x00Name");
        assert_eq!(meta.name, Some("BadName".to_string()));

        // Empty/whitespace only
        meta.set_name("   ");
        assert!(meta.name.is_none());

        // Too long name is truncated
        meta.set_name("A".repeat(1000));
        assert!(meta.name.as_ref().unwrap().len() <= validation::MAX_NAME_LENGTH);
    }

    #[test]
    fn test_add_tag_sanitizes() {
        let mut meta = BundleMetadata::default();

        // Normal tags
        meta.add_tag("robot");
        meta.add_tag("sci-fi");
        assert_eq!(meta.tags.len(), 2);

        // Empty tag is rejected
        meta.add_tag("");
        meta.add_tag("   ");
        assert_eq!(meta.tags.len(), 2);

        // Duplicate is rejected
        meta.add_tag("robot");
        assert_eq!(meta.tags.len(), 2);

        // Too long tag is truncated
        meta.add_tag("X".repeat(1000));
        assert_eq!(meta.tags.len(), 3);
        assert!(meta.tags[2].len() <= validation::MAX_TAG_LENGTH);

        // Can't exceed max tags
        for i in 0..200 {
            meta.add_tag(format!("tag{}", i));
        }
        assert_eq!(meta.tags.len(), validation::MAX_TAGS);
    }

    #[test]
    fn test_export_bundle_zip() {
        let tmp = tempfile::tempdir().unwrap();
        let bundle_dir = tmp.path().join("2026-02-22_120000");
        std::fs::create_dir_all(&bundle_dir).unwrap();

        // Create bundle files
        std::fs::write(bundle_dir.join(bundle_files::METADATA), r#"{"version":1}"#).unwrap();
        std::fs::write(bundle_dir.join(bundle_files::IMAGE), b"fake png").unwrap();
        std::fs::write(bundle_dir.join(bundle_files::MODEL_GLB), b"fake glb").unwrap();

        // Create subdirectory with texture
        let textures_dir = bundle_dir.join(bundle_files::TEXTURES_DIR);
        std::fs::create_dir_all(&textures_dir).unwrap();
        std::fs::write(textures_dir.join("texture_0.png"), b"fake texture").unwrap();

        let zip_path = tmp.path().join("test.zip");
        let count = export_bundle_zip(&bundle_dir, &zip_path).unwrap();
        assert_eq!(count, 4);

        // Verify zip contents
        let file = std::fs::File::open(&zip_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        assert_eq!(archive.len(), 4);

        let mut names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        names.sort();
        assert!(names.contains(&bundle_files::METADATA.to_string()));
        assert!(names.contains(&bundle_files::IMAGE.to_string()));
        assert!(names.contains(&bundle_files::MODEL_GLB.to_string()));
        assert!(names.contains(&"textures/texture_0.png".to_string()));

        // Verify file contents survived round-trip
        use std::io::Read;
        let mut buf = String::new();
        archive
            .by_name(bundle_files::METADATA)
            .unwrap()
            .read_to_string(&mut buf)
            .unwrap();
        assert_eq!(buf, r#"{"version":1}"#);
    }

    #[test]
    fn test_export_bundle_zip_nonexistent_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let result =
            export_bundle_zip(&tmp.path().join("nonexistent"), &tmp.path().join("out.zip"));
        assert!(result.is_err());
    }
}
