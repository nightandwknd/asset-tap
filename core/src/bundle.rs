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
//! - Artifact inventory and a linear pipeline of steps (v2)
//! - v1 `config` / `model_info` are read on load only, never written
//! - User tags and favorites
//! - Generation duration
//!
//! This module handles loading, saving, and error-tolerant discovery of bundles.
//!
//! # See Also
//!
//! - [`PipelineOutput`](crate::types::PipelineOutput) - Pipeline execution results
//! - [`history`](crate::history) - Generation history tracking

use crate::bundle_schema;
use crate::constants::files::DEMO_BUNDLE_URL;
use crate::constants::files::bundle as bundle_files;
use crate::constants::validation;
use crate::history::GenerationConfig;
use crate::state::ModelInfo;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// Metadata filename within each bundle directory.
const BUNDLE_METADATA_FILE: &str = bundle_files::METADATA;

/// Generator string stamped into every bundle created by this build.
const GENERATOR: &str = concat!("asset-tap/", env!("CARGO_PKG_VERSION"));

/// Maximum total decompressed size permitted when extracting a bundle zip.
/// Guards against zip bombs during import (extraction itself is otherwise
/// unbounded even though network downloads are size-capped).
const MAX_EXTRACT_TOTAL_BYTES: u64 = 1024 * 1024 * 1024; // 1 GiB

/// Maximum number of entries permitted in a bundle zip.
const MAX_EXTRACT_ENTRIES: usize = 10_000;

/// Returns the generator identifier for the current build (e.g. "asset-tap/26.3.6").
pub fn generator_string() -> &'static str {
    GENERATOR
}

/// Re-export standard file names for convenience.
pub mod files {
    pub use crate::constants::files::bundle::*;
}

pub use crate::bundle_schema::{
    Artifact, BundlePipeline, PipelineStep, SCHEMA_VERSION, modalities, ops, roles,
};

/// Extract model statistics (vertex/triangle count, file size) from a GLB file.
///
/// Parses the GLB without validation so stats are available even when a file
/// declares extensions the `gltf` crate doesn't support (e.g. trellis-2 uses
/// `EXT_texture_webp`, which `import_slice` rejects outright). We only read
/// accessor counts, so skipping texture/extension validation is safe here.
pub fn extract_model_info(glb_path: &Path) -> Option<ModelInfo> {
    let metadata = std::fs::metadata(glb_path).ok()?;
    let file_size = metadata.len();
    let format = glb_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("unknown")
        .to_uppercase();

    let glb_data = std::fs::read(glb_path).ok()?;
    let document = gltf::Gltf::from_slice_without_validation(&glb_data)
        .ok()?
        .document;

    let mut vertex_count: usize = 0;
    let mut triangle_count: usize = 0;

    for mesh in document.meshes() {
        for primitive in mesh.primitives() {
            // Vertex count from POSITION accessor
            if let Some(accessor) = primitive.get(&gltf::Semantic::Positions) {
                vertex_count += accessor.count();
            }

            // Triangle count from index accessor or vertex count
            if let Some(indices) = primitive.indices() {
                triangle_count += indices.count() / 3;
            } else if let Some(accessor) = primitive.get(&gltf::Semantic::Positions) {
                triangle_count += accessor.count() / 3;
            }
        }
    }

    Some(ModelInfo {
        file_size,
        format,
        vertex_count,
        triangle_count,
    })
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
    /// Present on v1 files; omitted on new writes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<GenerationConfig>,

    /// Model statistics (vertex count, file size, etc.).
    /// Present on v1 files; omitted on new writes (stats live on artifacts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_info: Option<ModelInfo>,

    /// Duration of generation in milliseconds.
    pub duration_ms: Option<u64>,

    /// User-defined tags for organization.
    pub tags: Vec<String>,

    /// Marked as favorite for quick access.
    pub favorite: bool,

    /// Notes or description added by user.
    pub notes: Option<String>,

    /// Generator identifier, e.g. "asset-tap/26.3.6".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generator: Option<String>,

    /// Demo bundle content version (e.g. 1, 2, 3).
    /// Only present on demo bundles downloaded from GitHub Releases.
    /// Incremented when demo content changes; used to detect duplicates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub demo_version: Option<u32>,

    /// Optional category. Omitted on write until a recipe can name the asset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,

    /// Inventory of files (and dropped intermediates) in this bundle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<Artifact>,

    /// Artifact id a viewer should open first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<String>,

    /// Ordered provenance: model calls and deterministic ops.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<BundlePipeline>,
}

/// Prompt, models, params, and mesh stats from v2 fields, or v1 fallback.
#[derive(Debug, Default, Clone)]
pub struct GenerationView {
    pub prompt: Option<String>,
    pub user_prompt: Option<String>,
    pub template: Option<String>,
    pub image_model: Option<String>,
    pub model_3d: Option<String>,
    pub image_params: HashMap<String, Value>,
    pub model_3d_params: HashMap<String, Value>,
    pub vertex_count: Option<usize>,
    pub triangle_count: Option<usize>,
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
            generator: None,
            demo_version: None,
            category: None,
            artifacts: Vec::new(),
            primary: None,
            pipeline: None,
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
            generator: Some(GENERATOR.to_string()),
            ..Default::default()
        }
    }

    /// Build v2 metadata for a finished generation, from files already on disk.
    ///
    /// Writes `artifacts` / `pipeline` only. v1 `config` / `model_info` are
    /// omitted. Does not rewrite existing v1 bundles.
    pub fn for_generation(
        bundle_dir: &Path,
        config: GenerationConfig,
        model_info: Option<ModelInfo>,
        image_provider_id: Option<&str>,
        model_3d_provider_id: Option<&str>,
    ) -> Self {
        let manifest = bundle_schema::GenerationManifest {
            config,
            model_info,
            image_provider_id: image_provider_id.map(str::to_string),
            model_3d_provider_id: model_3d_provider_id.map(str::to_string),
        };
        let (artifacts, primary, pipeline) =
            bundle_schema::describe_generation(bundle_dir, &manifest);
        Self {
            version: SCHEMA_VERSION,
            generator: Some(GENERATOR.to_string()),
            artifacts,
            primary,
            pipeline: Some(pipeline),
            ..Default::default()
        }
    }

    /// Prompt, models, params, and mesh stats — pipeline/artifacts first,
    /// v1 `config` / `model_info` only to fill holes.
    pub fn generation_view(&self) -> GenerationView {
        let (artifacts, _, _, pipeline) = self.v2_view();
        let mut out = GenerationView::default();
        for step in &pipeline.steps {
            let PipelineStep::Model {
                model,
                modality,
                prompt,
                user_prompt,
                template,
                params,
                ..
            } = step
            else {
                continue;
            };
            match modality.as_str() {
                bundle_schema::modalities::TEXT_TO_IMAGE => {
                    if out.image_model.is_none() {
                        out.image_model = Some(model.clone());
                    }
                    if out.prompt.is_none() {
                        out.prompt = prompt.clone();
                    }
                    if out.user_prompt.is_none() {
                        out.user_prompt = user_prompt.clone();
                    }
                    if out.template.is_none() {
                        out.template = template.clone();
                    }
                    if out.image_params.is_empty() {
                        out.image_params = params.clone();
                    }
                }
                bundle_schema::modalities::IMAGE_TO_3D | bundle_schema::modalities::TEXT_TO_3D => {
                    if out.model_3d.is_none() {
                        out.model_3d = Some(model.clone());
                    }
                    if out.prompt.is_none() {
                        out.prompt = prompt.clone();
                    }
                    if out.user_prompt.is_none() {
                        out.user_prompt = user_prompt.clone();
                    }
                    if out.template.is_none() {
                        out.template = template.clone();
                    }
                    if out.model_3d_params.is_empty() {
                        out.model_3d_params = params.clone();
                    }
                }
                _ => {}
            }
        }
        if let Some(cfg) = &self.config {
            if out.prompt.is_none() {
                out.prompt = cfg.prompt.clone();
            }
            if out.user_prompt.is_none() {
                out.user_prompt = cfg.user_prompt.clone();
            }
            if out.template.is_none() {
                out.template = cfg.template.clone();
            }
            if out.image_model.is_none() {
                out.image_model = cfg.image_model.clone();
            }
            if out.model_3d.is_none() && !cfg.model_3d.is_empty() {
                out.model_3d = Some(cfg.model_3d.clone());
            }
            if out.image_params.is_empty() {
                out.image_params = cfg.image_model_params.clone();
            }
            if out.model_3d_params.is_empty() {
                out.model_3d_params = cfg.model_3d_params.clone();
            }
        }
        for art in &artifacts {
            if art.role == roles::MODEL {
                if out.vertex_count.is_none() {
                    out.vertex_count = art.vertex_count;
                }
                if out.triangle_count.is_none() {
                    out.triangle_count = art.triangle_count;
                }
            }
        }
        if let Some(info) = &self.model_info {
            if out.vertex_count.is_none() {
                out.vertex_count = Some(info.vertex_count);
            }
            if out.triangle_count.is_none() {
                out.triangle_count = Some(info.triangle_count);
            }
        }
        out
    }

    /// Full v2 projection: stored fields, or synthesized from v1 `config`.
    ///
    /// In-memory only — never used to rewrite a v1 file on load.
    pub fn v2_view(
        &self,
    ) -> (
        Vec<Artifact>,
        Option<String>,
        Option<String>,
        BundlePipeline,
    ) {
        if !self.artifacts.is_empty() {
            return (
                self.artifacts.clone(),
                self.primary.clone(),
                self.category.clone(),
                self.pipeline.clone().unwrap_or_default(),
            );
        }
        let (artifacts, primary, pipeline) =
            bundle_schema::synthesize_from_v1(self.config.as_ref(), self.model_info.as_ref());
        (artifacts, primary, None, pipeline)
    }

    /// Artifact inventory as stored, or synthesized from v1 `config`.
    pub fn artifact_inventory(&self) -> Vec<Artifact> {
        self.v2_view().0
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

        let contents = serde_json::to_vec_pretty(self)
            .map_err(|e| BundleError::Serialization { source: e })?;

        // Atomic write so an interrupted save can't corrupt an existing
        // bundle.json (favorites, tags, notes are edited in place).
        crate::config::atomic_write(
            &path,
            &contents,
            crate::config::AtomicWriteOptions::default(),
        )
        .map_err(|e| BundleError::Io { path, source: e })
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
        if let Some(duration) = self.duration_ms
            && duration > validation::MAX_DURATION_MS
        {
            issues.push(format!(
                "Duration {} ms exceeds maximum, clamping",
                duration
            ));
            self.duration_ms = Some(validation::MAX_DURATION_MS);
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

/// Download timeout for the demo bundle (2 minutes for ~34 MB).
const DEMO_DOWNLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Compute the SHA-256 hash of a byte slice, returned as a lowercase hex string.
pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(data))
}

/// Verify that the SHA-256 hash of `data` matches `expected_hex`.
fn verify_sha256(data: &[u8], expected_hex: &str) -> anyhow::Result<()> {
    let actual = sha256_hex(data);
    if actual != expected_hex {
        anyhow::bail!(
            "Integrity check failed: expected {}, got {}",
            expected_hex,
            actual
        );
    }
    Ok(())
}

/// Result of a demo bundle download attempt.
#[derive(Debug)]
pub enum DemoDownloadResult {
    /// New demo bundle downloaded and extracted.
    Downloaded(PathBuf),
    /// This demo version already exists locally.
    AlreadyExists(u32),
}

/// Check whether a specific demo bundle version already exists locally.
pub fn has_demo_version(output_dir: &Path, version: u32) -> bool {
    discover_bundles(output_dir)
        .iter()
        .any(|b| b.metadata.demo_version == Some(version))
}

/// Download the demo bundle from GitHub Releases to the output directory.
///
/// First fetches a small manifest to check the demo version. If that version
/// already exists locally, returns [`DemoDownloadResult::AlreadyExists`] without
/// downloading the full archive.
///
/// Otherwise, downloads the `.zip` archive containing `image.png`, `model.glb`,
/// and `bundle.json`, then extracts them into a new timestamped directory.
///
/// The download + extraction is atomic: files are extracted to a temporary
/// directory first and only renamed to the final path on success.
///
/// The `on_progress` callback receives values from 0.0 to 1.0 indicating
/// download progress when `Content-Length` is available, or -1.0 to signal
/// indeterminate progress (download active but total size unknown).
pub async fn download_demo_bundle(
    output_dir: PathBuf,
    on_progress: impl Fn(f32) + Send + 'static,
) -> anyhow::Result<DemoDownloadResult> {
    use crate::constants::files::DEMO_MANIFEST_URL;

    let client = reqwest::Client::builder()
        .timeout(DEMO_DOWNLOAD_TIMEOUT)
        .build()?;

    // Phase 1: fetch the manifest to check the demo version.
    info!("Checking demo bundle version...");
    let manifest_resp = client.get(DEMO_MANIFEST_URL).send().await?;
    if !manifest_resp.status().is_success() {
        anyhow::bail!(
            "Failed to fetch demo manifest: HTTP {}",
            manifest_resp.status()
        );
    }

    let manifest: serde_json::Value = manifest_resp.json().await?;
    let demo_version = manifest
        .get("demo_version")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| anyhow::anyhow!("Demo manifest missing demo_version field"))?;

    // Check if this version already exists locally.
    if has_demo_version(&output_dir, demo_version) {
        info!(
            "Demo bundle v{} already exists, skipping download",
            demo_version
        );
        return Ok(DemoDownloadResult::AlreadyExists(demo_version));
    }

    // Phase 2: download the full zip.
    info!(
        "Downloading demo bundle v{} from {}",
        demo_version, DEMO_BUNDLE_URL
    );
    let response = client.get(DEMO_BUNDLE_URL).send().await?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to download demo bundle: HTTP {}", response.status());
    }

    let total_size = response.content_length();
    let mut downloaded: u64 = 0;
    let mut bytes = Vec::new();

    let mut stream = response.bytes_stream();
    use futures::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        downloaded += chunk.len() as u64;
        bytes.extend_from_slice(&chunk);

        match total_size {
            Some(total) => on_progress(downloaded as f32 / total as f32),
            None => on_progress(-1.0),
        }
    }

    on_progress(1.0);
    info!("Downloaded {} bytes, verifying integrity...", bytes.len());

    // Phase 3: verify SHA-256 integrity. Fail closed — a manifest without a
    // hash is treated as an error rather than silently skipping verification,
    // so a manifest-only compromise or a workflow regression can't disable the
    // integrity check unnoticed.
    let expected_hash = manifest
        .get("sha256")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Demo manifest is missing a sha256 hash; refusing to install unverified download"
            )
        })?;
    verify_sha256(&bytes, expected_hash)?;
    info!("SHA-256 integrity verified");

    // Create a timestamped directory like normal bundles, with collision
    // suffix if another bundle landed in the same second.
    let target_dir = crate::config::unique_timestamped_path(&output_dir);

    // Extract to a temporary directory first, then rename atomically.
    let final_target = target_dir.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        std::fs::create_dir_all(&output_dir)?;

        let tmp_dir = tempfile::tempdir_in(&output_dir)?;

        let cursor = std::io::Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor)?;
        extract_zip_to_dir(&mut archive, tmp_dir.path()).map_err(|e| anyhow::anyhow!("{}", e))?;

        // Atomic rename: move temp dir to the final target path.
        let tmp_path = tmp_dir.keep();
        std::fs::rename(&tmp_path, &final_target)?;

        Ok(())
    })
    .await??;

    info!(
        "Demo bundle v{} extracted to {}",
        demo_version,
        target_dir.display()
    );

    Ok(DemoDownloadResult::Downloaded(target_dir))
}

/// Check whether a directory looks like a bundle.
///
/// A bundle needs at least one of `bundle.json`, `image.png`, or `model.glb`
/// to be considered non-empty. Used by the GUI bundle browser to skip
/// partial/interrupted generations that would otherwise show as empty entries.
pub fn looks_like_bundle(path: &Path) -> bool {
    path.join(bundle_files::METADATA).exists()
        || path.join(bundle_files::IMAGE).exists()
        || path.join(bundle_files::MODEL_GLB).exists()
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
    if let Some(name) = bundle_dir.file_name().and_then(|n| n.to_str())
        && let Some(dt) = parse_timestamp_dir_name(name)
    {
        metadata.created_at = dt;
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

/// Import a bundle from a zip archive into the output directory.
///
/// Extracts the zip contents into a new timestamped directory. If the zip
/// contains a top-level folder, files are flattened (same as demo download).
/// The extracted bundle must contain at least an image or model to be valid.
/// If no `bundle.json` is present, metadata is inferred from the directory.
///
/// Extraction is atomic: files go to a temp directory first and are only
/// renamed to the final path on success.
///
/// Returns the path to the new bundle directory.
pub fn import_bundle_zip(source_zip: &Path, output_dir: &Path) -> Result<PathBuf, String> {
    let zip_file =
        std::fs::File::open(source_zip).map_err(|e| format!("Failed to open zip: {}", e))?;
    let mut archive =
        zip::ZipArchive::new(zip_file).map_err(|e| format!("Invalid zip archive: {}", e))?;

    std::fs::create_dir_all(output_dir)
        .map_err(|e| format!("Failed to create output directory: {}", e))?;

    let tmp_dir = tempfile::tempdir_in(output_dir)
        .map_err(|e| format!("Failed to create temp directory: {}", e))?;

    let file_count = extract_zip_to_dir(&mut archive, tmp_dir.path())?;

    if file_count == 0 {
        return Err("Zip archive is empty".to_string());
    }

    finalize_imported_bundle(tmp_dir, output_dir, file_count)
}

/// Import a bundle from a plain directory (e.g. a CLI run's output folder) —
/// no archive step needed. Copies the directory's contents into a new
/// timestamped bundle under `output_dir`, with the same validation and
/// metadata inference as zip import. The source directory is left untouched.
pub fn import_bundle_dir(source_dir: &Path, output_dir: &Path) -> Result<PathBuf, String> {
    if !source_dir.is_dir() {
        return Err(format!("Not a directory: {}", source_dir.display()));
    }

    std::fs::create_dir_all(output_dir)
        .map_err(|e| format!("Failed to create output directory: {}", e))?;

    // Refuse to import a bundle into itself (source inside output_dir is fine —
    // that's re-importing a library bundle — but identical paths are not).
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    if canon(source_dir) == canon(output_dir) {
        return Err("Source directory is the output directory itself".to_string());
    }

    let tmp_dir = tempfile::tempdir_in(output_dir)
        .map_err(|e| format!("Failed to create temp directory: {}", e))?;

    let mut file_count = 0usize;
    copy_dir_contents(source_dir, tmp_dir.path(), &mut file_count)?;

    if file_count == 0 {
        return Err("Directory is empty".to_string());
    }

    finalize_imported_bundle(tmp_dir, output_dir, file_count)
}

/// Recursively copy a directory's contents, skipping macOS junk files and
/// enforcing the same entry cap as zip extraction.
fn copy_dir_contents(src: &Path, dest: &Path, count: &mut usize) -> Result<(), String> {
    for entry in std::fs::read_dir(src).map_err(|e| format!("Failed to read directory: {}", e))? {
        let entry = entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if is_macos_archive_junk(&name_str) {
            continue;
        }
        let src_path = entry.path();
        let dest_path = dest.join(&name);
        let file_type = entry
            .file_type()
            .map_err(|e| format!("Failed to stat {}: {}", src_path.display(), e))?;
        // Symlinks are skipped: a link could point anywhere (including outside
        // the bundle); imports copy real content only, like zip extraction.
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            std::fs::create_dir_all(&dest_path)
                .map_err(|e| format!("Failed to create {}: {}", dest_path.display(), e))?;
            copy_dir_contents(&src_path, &dest_path, count)?;
        } else {
            if *count >= MAX_EXTRACT_ENTRIES {
                return Err(format!(
                    "Directory has too many files (max {MAX_EXTRACT_ENTRIES})"
                ));
            }
            std::fs::copy(&src_path, &dest_path)
                .map_err(|e| format!("Failed to copy {}: {}", src_path.display(), e))?;
            *count += 1;
        }
    }
    Ok(())
}

/// Shared tail of bundle import: validate the staged contents, infer metadata
/// when bundle.json is absent, and atomically move the staging directory to
/// its final timestamped home.
fn finalize_imported_bundle(
    tmp_dir: tempfile::TempDir,
    output_dir: &Path,
    file_count: usize,
) -> Result<PathBuf, String> {
    // Validate: must contain at least an image or model.
    let mut contents = BundleContents::default();
    let mut issues = Vec::new();
    scan_bundle_contents(tmp_dir.path(), &mut contents, &mut issues);

    if !contents.has_content() {
        return Err(
            "Bundle must contain at least an image (image.png) or model (model.glb)".to_string(),
        );
    }

    // If no bundle.json, create inferred metadata so the bundle has a name.
    let metadata_path = tmp_dir.path().join(bundle_files::METADATA);
    if !metadata_path.exists() {
        let metadata = BundleMetadata::default();
        if let Err(e) = metadata.save(tmp_dir.path()) {
            warn!("Failed to write inferred metadata: {}", e);
        }
    }

    // Atomic rename to final timestamped directory, with collision suffix
    // if another bundle landed in the same second.
    let final_dir = crate::config::unique_timestamped_path(output_dir);
    let tmp_path = tmp_dir.keep();
    std::fs::rename(&tmp_path, &final_dir)
        .map_err(|e| format!("Failed to finalize bundle: {}", e))?;

    info!(
        "Imported bundle ({} files) to {}",
        file_count,
        final_dir.display()
    );

    Ok(final_dir)
}

/// macOS Archive Utility (Finder's "Compress") pollutes zips with AppleDouble
/// metadata: a parallel `__MACOSX/` tree, `._*` resource-fork files, and
/// `.DS_Store`. None of it is bundle content — and the `__MACOSX/` tree breaks
/// wrapper-folder detection (two top-level dirs → no common prefix), which
/// made every Archive-Utility-zipped bundle fail import validation.
fn is_macos_archive_junk(path: &str) -> bool {
    if path.split('/').next() == Some("__MACOSX") {
        return true;
    }
    match path.rsplit('/').next() {
        Some(name) => name == ".DS_Store" || name.starts_with("._"),
        None => false,
    }
}

/// Extract files from a zip archive into a destination directory.
///
/// If all files share a common top-level directory (e.g., `bundle-name/image.png`),
/// that prefix is stripped. Subdirectory structure below the prefix is preserved
/// (e.g., `bundle-name/textures/base.png` → `textures/base.png`).
///
/// Returns the number of files extracted.
fn extract_zip_to_dir<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    dest: &Path,
) -> Result<usize, String> {
    use std::io::Read as _;

    if archive.len() > MAX_EXTRACT_ENTRIES {
        return Err(format!(
            "Archive has too many entries ({}, max {})",
            archive.len(),
            MAX_EXTRACT_ENTRIES
        ));
    }

    // First pass: collect the *sanitized* file paths and detect common prefix.
    // `safe_zip_entry_path` rejects absolute paths, `..` traversal, and other
    // names that would escape `dest` (zip-slip); such entries are skipped here
    // and again in the extraction pass below.
    let mut paths: Vec<String> = Vec::new();
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| format!("Failed to read zip entry: {}", e))?;
        if entry.is_file()
            && let Some(safe) = safe_zip_entry_path(&entry)
            && !is_macos_archive_junk(&safe)
        {
            paths.push(safe);
        }
    }

    // Detect common single-directory prefix (e.g., "asset-tap/" or "My Bundle/").
    let prefix = detect_common_prefix(&paths);

    // Second pass: extract files, stripping the common prefix.
    let mut file_count = 0;
    let mut total_bytes: u64 = 0;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("Failed to read zip entry: {}", e))?;

        if !entry.is_file() {
            continue;
        }

        // Reject unsafe entry names outright rather than silently skipping, so a
        // crafted archive that mixes safe and traversal entries can't partially
        // extract. A None here means the name escapes the destination directory.
        let Some(raw_path) = safe_zip_entry_path(&entry) else {
            return Err(format!(
                "Refusing to extract unsafe zip entry path: {:?}",
                entry.name()
            ));
        };

        // Archive-Utility metadata is not bundle content — skip it entirely.
        if is_macos_archive_junk(&raw_path) {
            continue;
        }

        let relative = if let Some(ref pfx) = prefix {
            raw_path.strip_prefix(pfx).unwrap_or(&raw_path)
        } else {
            &raw_path
        };

        if relative.is_empty() {
            continue;
        }

        let dest_path = dest.join(relative);

        // Defense in depth: verify the joined path still lands under `dest`.
        // `safe_zip_entry_path` already guarantees this, but re-check against
        // the actual join so the invariant is enforced at the write site.
        if !dest_path.starts_with(dest) {
            return Err(format!(
                "Refusing to extract zip entry outside destination: {:?}",
                relative
            ));
        }

        // Create parent directories (for textures/ etc.)
        if let Some(parent) = dest_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory: {}", e))?;
        }

        // Bound total decompressed bytes (zip-bomb guard). Copy through a
        // limited reader so a lying/absent Content-Length can't exhaust disk.
        let remaining = MAX_EXTRACT_TOTAL_BYTES.saturating_sub(total_bytes);
        let mut file = std::fs::File::create(&dest_path)
            .map_err(|e| format!("Failed to create file: {}", e))?;
        let written = std::io::copy(&mut (&mut entry).take(remaining + 1), &mut file)
            .map_err(|e| format!("Failed to extract file: {}", e))?;
        total_bytes = total_bytes.saturating_add(written);
        if total_bytes > MAX_EXTRACT_TOTAL_BYTES {
            return Err(format!(
                "Archive exceeds maximum decompressed size ({} bytes)",
                MAX_EXTRACT_TOTAL_BYTES
            ));
        }
        file_count += 1;
    }

    Ok(file_count)
}

/// Return an entry's path as a forward-slashed relative string if — and only
/// if — it is safe to join onto a destination directory.
///
/// Uses `zip`'s `enclosed_name()`, which returns `None` for absolute paths,
/// paths containing `..` components, and (on Windows) drive-letter/UNC
/// prefixes — the zip-slip attack surface. Normalizes the resulting path to
/// `/` separators so downstream prefix detection is platform-independent.
fn safe_zip_entry_path(entry: &zip::read::ZipFile<'_>) -> Option<String> {
    let enclosed = entry.enclosed_name()?;
    let mut parts = Vec::new();
    for component in enclosed.components() {
        match component {
            std::path::Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            // enclosed_name() already guarantees these are absent, but reject
            // defensively rather than silently dropping a component.
            std::path::Component::CurDir => continue,
            _ => return None,
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

/// Detect a common single-directory prefix shared by all paths.
///
/// Returns `Some("dir/")` if all paths start with the same directory component,
/// or `None` if there is no common prefix (files are at the root).
fn detect_common_prefix(paths: &[String]) -> Option<String> {
    if paths.is_empty() {
        return None;
    }

    // Get the first path component of each entry.
    let first_components: Vec<Option<&str>> = paths
        .iter()
        .map(|p| p.split('/').next().filter(|c| !c.is_empty()))
        .collect();

    // Check if all entries share the same first component AND that component
    // is actually a directory prefix (i.e., no file lives at the root level).
    if let Some(Some(common)) = first_components.first() {
        let all_match = first_components
            .iter()
            .all(|c| c.map(|v| v == *common).unwrap_or(false));
        let all_nested = paths.iter().all(|p| p.contains('/'));

        if all_match && all_nested {
            return Some(format!("{common}/"));
        }
    }

    None
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
        // Zip entry names must use forward slashes per the spec. On Windows
        // `to_string_lossy()` yields backslashes, which produce non-portable
        // archives and break the `/`-based prefix detection on re-import.
        let relative = path
            .strip_prefix(base)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");

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
    fn test_looks_like_bundle() {
        let tmp = tempfile::tempdir().unwrap();

        // Empty dir → not a bundle
        let empty = tmp.path().join("empty");
        std::fs::create_dir(&empty).unwrap();
        assert!(!looks_like_bundle(&empty));

        // Dir with only an unrelated file → not a bundle
        let junk = tmp.path().join("junk");
        std::fs::create_dir(&junk).unwrap();
        std::fs::write(junk.join("notes.txt"), "hi").unwrap();
        assert!(!looks_like_bundle(&junk));

        // Any one of the three bundle indicators is enough
        for filename in [
            bundle_files::METADATA,
            bundle_files::IMAGE,
            bundle_files::MODEL_GLB,
        ] {
            let dir = tmp.path().join(format!("bundle_{}", filename));
            std::fs::create_dir(&dir).unwrap();
            std::fs::write(dir.join(filename), b"").unwrap();
            assert!(looks_like_bundle(&dir), "expected {} to qualify", filename);
        }
    }

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

    #[test]
    fn test_extract_model_info_nonexistent_file() {
        let result = extract_model_info(Path::new("/nonexistent/model.glb"));
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_model_info_invalid_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("invalid.glb");
        std::fs::write(&path, b"not a valid glb").unwrap();
        let result = extract_model_info(&path);
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_model_info_valid_glb() {
        // Build a minimal valid GLB with a single triangle (3 vertices, 1 triangle)
        let gltf_json = serde_json::json!({
            "asset": { "version": "2.0" },
            "scene": 0,
            "scenes": [{ "nodes": [0] }],
            "nodes": [{ "mesh": 0 }],
            "meshes": [{
                "primitives": [{
                    "attributes": { "POSITION": 0 },
                    "indices": 1
                }]
            }],
            "accessors": [
                {
                    "bufferView": 0,
                    "componentType": 5126,
                    "count": 3,
                    "type": "VEC3",
                    "max": [1.0, 1.0, 0.0],
                    "min": [0.0, 0.0, 0.0]
                },
                {
                    "bufferView": 1,
                    "componentType": 5123,
                    "count": 3,
                    "type": "SCALAR"
                }
            ],
            "bufferViews": [
                { "buffer": 0, "byteOffset": 0, "byteLength": 36, "target": 34962 },
                { "buffer": 0, "byteOffset": 36, "byteLength": 6, "target": 34963 }
            ],
            "buffers": [{ "byteLength": 44 }]
        });

        let json_bytes = serde_json::to_vec(&gltf_json).unwrap();
        // Pad JSON to 4-byte alignment
        let json_padded_len = (json_bytes.len() + 3) & !3;
        let mut json_chunk = json_bytes.clone();
        json_chunk.resize(json_padded_len, 0x20); // pad with spaces

        // Binary data: 3 vertices (3 * 3 * 4 = 36 bytes) + 3 indices (3 * 2 = 6 bytes) + 2 pad
        let mut bin_data: Vec<u8> = Vec::new();
        // Vertex 0: (0, 0, 0)
        bin_data.extend_from_slice(&0.0f32.to_le_bytes());
        bin_data.extend_from_slice(&0.0f32.to_le_bytes());
        bin_data.extend_from_slice(&0.0f32.to_le_bytes());
        // Vertex 1: (1, 0, 0)
        bin_data.extend_from_slice(&1.0f32.to_le_bytes());
        bin_data.extend_from_slice(&0.0f32.to_le_bytes());
        bin_data.extend_from_slice(&0.0f32.to_le_bytes());
        // Vertex 2: (0, 1, 0)
        bin_data.extend_from_slice(&0.0f32.to_le_bytes());
        bin_data.extend_from_slice(&1.0f32.to_le_bytes());
        bin_data.extend_from_slice(&0.0f32.to_le_bytes());
        // Indices: 0, 1, 2
        bin_data.extend_from_slice(&0u16.to_le_bytes());
        bin_data.extend_from_slice(&1u16.to_le_bytes());
        bin_data.extend_from_slice(&2u16.to_le_bytes());
        // Pad to 4-byte alignment
        bin_data.resize((bin_data.len() + 3) & !3, 0);

        // Build GLB: header + JSON chunk + BIN chunk
        let total_len = 12 + 8 + json_chunk.len() + 8 + bin_data.len();
        let mut glb = Vec::with_capacity(total_len);
        // GLB header
        glb.extend_from_slice(b"glTF");
        glb.extend_from_slice(&2u32.to_le_bytes()); // version
        glb.extend_from_slice(&(total_len as u32).to_le_bytes());
        // JSON chunk
        glb.extend_from_slice(&(json_chunk.len() as u32).to_le_bytes());
        glb.extend_from_slice(&0x4E4F534Au32.to_le_bytes()); // "JSON"
        glb.extend_from_slice(&json_chunk);
        // BIN chunk
        glb.extend_from_slice(&(bin_data.len() as u32).to_le_bytes());
        glb.extend_from_slice(&0x004E4942u32.to_le_bytes()); // "BIN\0"
        glb.extend_from_slice(&bin_data);

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.glb");
        std::fs::write(&path, &glb).unwrap();

        let info = extract_model_info(&path).expect("Should parse valid GLB");
        assert_eq!(info.vertex_count, 3);
        assert_eq!(info.triangle_count, 1);
        assert_eq!(info.format, "GLB");
        assert!(info.file_size > 0);
    }

    #[test]
    fn test_has_demo_version_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!has_demo_version(tmp.path(), 1));
    }

    #[test]
    fn test_has_demo_version_finds_matching() {
        let tmp = tempfile::tempdir().unwrap();
        let bundle_dir = tmp.path().join("2026-04-02_000000");
        std::fs::create_dir(&bundle_dir).unwrap();
        std::fs::write(bundle_dir.join("image.png"), b"fake-png").unwrap();
        let metadata = serde_json::json!({
            "version": 1,
            "name": "Test Demo",
            "demo_version": 1,
        });
        std::fs::write(
            bundle_dir.join("bundle.json"),
            serde_json::to_string_pretty(&metadata).unwrap(),
        )
        .unwrap();

        assert!(has_demo_version(tmp.path(), 1));
        assert!(!has_demo_version(tmp.path(), 2));
    }

    #[test]
    fn test_has_demo_version_ignores_non_demo() {
        let tmp = tempfile::tempdir().unwrap();
        let bundle_dir = tmp.path().join("2026-04-02_000000");
        std::fs::create_dir(&bundle_dir).unwrap();
        std::fs::write(bundle_dir.join("image.png"), b"fake-png").unwrap();
        // Normal bundle without demo_version
        let metadata = serde_json::json!({
            "version": 1,
            "name": "My Generation",
        });
        std::fs::write(
            bundle_dir.join("bundle.json"),
            serde_json::to_string_pretty(&metadata).unwrap(),
        )
        .unwrap();

        assert!(!has_demo_version(tmp.path(), 1));
    }

    /// Helper: create a zip archive in memory with the given file entries.
    fn create_test_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let options = zip::write::SimpleFileOptions::default();
            for (name, data) in files {
                zip.start_file(*name, options).unwrap();
                use std::io::Write;
                zip.write_all(data).unwrap();
            }
            zip.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn test_import_bundle_zip_valid() {
        let tmp = tempfile::tempdir().unwrap();
        let zip_path = tmp.path().join("test.zip");
        let zip_data = create_test_zip(&[("image.png", b"fake-png"), ("model.glb", b"fake-glb")]);
        std::fs::write(&zip_path, &zip_data).unwrap();

        let output_dir = tmp.path().join("output");
        let result = import_bundle_zip(&zip_path, &output_dir);
        assert!(result.is_ok());

        let bundle_dir = result.unwrap();
        assert!(bundle_dir.join("image.png").exists());
        assert!(bundle_dir.join("model.glb").exists());
        assert!(bundle_dir.join("bundle.json").exists()); // Inferred metadata created
    }

    #[test]
    fn test_import_bundle_zip_with_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let zip_path = tmp.path().join("test.zip");
        let zip_data = create_test_zip(&[
            ("My Bundle/image.png", b"fake-png"),
            ("My Bundle/model.glb", b"fake-glb"),
            ("My Bundle/bundle.json", b"{}"),
        ]);
        std::fs::write(&zip_path, &zip_data).unwrap();

        let output_dir = tmp.path().join("output");
        let result = import_bundle_zip(&zip_path, &output_dir);
        assert!(result.is_ok());

        let bundle_dir = result.unwrap();
        // Prefix should be stripped
        assert!(bundle_dir.join("image.png").exists());
        assert!(bundle_dir.join("model.glb").exists());
    }

    #[test]
    fn test_import_bundle_zip_preserves_textures_subdir() {
        let tmp = tempfile::tempdir().unwrap();
        let zip_path = tmp.path().join("test.zip");
        let zip_data = create_test_zip(&[
            ("bundle/image.png", b"fake-png"),
            ("bundle/model.glb", b"fake-glb"),
            ("bundle/textures/base_color.png", b"fake-texture"),
        ]);
        std::fs::write(&zip_path, &zip_data).unwrap();

        let output_dir = tmp.path().join("output");
        let result = import_bundle_zip(&zip_path, &output_dir);
        assert!(result.is_ok());

        let bundle_dir = result.unwrap();
        assert!(bundle_dir.join("image.png").exists());
        assert!(bundle_dir.join("textures/base_color.png").exists());
    }

    #[test]
    fn test_import_bundle_zip_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let zip_path = tmp.path().join("test.zip");
        let zip_data = create_test_zip(&[]);
        std::fs::write(&zip_path, &zip_data).unwrap();

        let output_dir = tmp.path().join("output");
        let result = import_bundle_zip(&zip_path, &output_dir);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty"));
    }

    #[test]
    fn test_import_rejects_parent_dir_traversal() {
        // A crafted archive with a `../` entry must not write outside the
        // destination. The whole import is rejected rather than partially
        // extracted (zip-slip).
        let tmp = tempfile::tempdir().unwrap();
        let zip_path = tmp.path().join("evil.zip");
        let zip_data =
            create_test_zip(&[("image.png", b"fake-png"), ("../../escaped.txt", b"pwned")]);
        std::fs::write(&zip_path, &zip_data).unwrap();

        let output_dir = tmp.path().join("output");
        let result = import_bundle_zip(&zip_path, &output_dir);
        assert!(result.is_err(), "traversal archive should be rejected");
        assert!(result.unwrap_err().contains("unsafe"));

        // The escape target must not exist anywhere near the temp root.
        assert!(!tmp.path().join("escaped.txt").exists());
        assert!(!tmp.path().parent().unwrap().join("escaped.txt").exists());
    }

    #[test]
    fn test_import_rejects_absolute_path_entry() {
        // On Unix, `dest.join("/abs")` discards `dest` entirely; enclosed_name()
        // rejects absolute entries so this must error, not escape.
        let tmp = tempfile::tempdir().unwrap();
        let zip_path = tmp.path().join("evil.zip");
        let abs_target = tmp.path().join("abs-escape.txt");
        let entry_name = format!("{}", abs_target.display());
        let zip_data =
            create_test_zip(&[("image.png", b"fake-png"), (entry_name.as_str(), b"pwned")]);
        std::fs::write(&zip_path, &zip_data).unwrap();

        let output_dir = tmp.path().join("output");
        // Either the entry is rejected as unsafe, or (if the OS/zip normalized
        // it to relative) it lands inside the bundle dir — never at the abs path.
        match import_bundle_zip(&zip_path, &output_dir) {
            Ok(_) => assert!(!abs_target.exists()),
            Err(e) => assert!(e.contains("unsafe")),
        }
    }

    #[test]
    fn test_import_bundle_zip_no_valid_assets() {
        let tmp = tempfile::tempdir().unwrap();
        let zip_path = tmp.path().join("test.zip");
        let zip_data = create_test_zip(&[("readme.txt", b"hello")]);
        std::fs::write(&zip_path, &zip_data).unwrap();

        let output_dir = tmp.path().join("output");
        let result = import_bundle_zip(&zip_path, &output_dir);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("image"));
    }

    /// Regression: macOS Archive Utility (Finder "Compress") zips carry a
    /// parallel `__MACOSX/` tree + `._*` AppleDouble files. The extra
    /// top-level dir used to break wrapper-folder flattening, so every
    /// Archive-Utility-zipped bundle failed import with "must contain at
    /// least an image or model". Entry list modeled on a real archive.
    #[test]
    fn test_import_bundle_zip_tolerates_macos_archive_junk() {
        let tmp = tempfile::tempdir().unwrap();
        let zip_path = tmp.path().join("finder.zip");
        let zip_data = create_test_zip(&[
            ("2026-08-20_022113/bundle.json", b"{}"),
            ("2026-08-20_022113/image.png", b"fake-png"),
            ("2026-08-20_022113/model.glb", b"fake-glb"),
            ("2026-08-20_022113/.DS_Store", b"junk"),
            ("__MACOSX/2026-08-20_022113/._bundle.json", b"junk"),
            ("__MACOSX/2026-08-20_022113/._image.png", b"junk"),
            ("__MACOSX/2026-08-20_022113/._model.glb", b"junk"),
        ]);
        std::fs::write(&zip_path, &zip_data).unwrap();

        let output_dir = tmp.path().join("output");
        let bundle_dir = import_bundle_zip(&zip_path, &output_dir)
            .expect("Archive Utility zips must import cleanly");

        // Wrapper stripped, content present, junk absent.
        assert!(bundle_dir.join("image.png").exists());
        assert!(bundle_dir.join("model.glb").exists());
        assert!(!bundle_dir.join(".DS_Store").exists());
        assert!(!bundle_dir.join("__MACOSX").exists());
        assert!(!bundle_dir.join("._image.png").exists());
    }

    #[test]
    fn test_is_macos_archive_junk() {
        assert!(is_macos_archive_junk("__MACOSX/bundle/._image.png"));
        assert!(is_macos_archive_junk("bundle/.DS_Store"));
        assert!(is_macos_archive_junk(".DS_Store"));
        assert!(is_macos_archive_junk("bundle/._model.glb"));
        assert!(!is_macos_archive_junk("bundle/image.png"));
        assert!(!is_macos_archive_junk("image.png"));
        // A real file that merely starts with underscore is not junk.
        assert!(!is_macos_archive_junk("bundle/_notes.txt"));
    }

    #[test]
    fn test_import_bundle_dir_copies_and_validates() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("cli-run");
        std::fs::create_dir_all(src.join("textures")).unwrap();
        std::fs::write(src.join("bundle.json"), b"{}").unwrap();
        std::fs::write(src.join("image.png"), b"fake-png").unwrap();
        std::fs::write(src.join("model.glb"), b"fake-glb").unwrap();
        std::fs::write(src.join("textures/base.png"), b"fake-tex").unwrap();
        std::fs::write(src.join(".DS_Store"), b"junk").unwrap();

        let output_dir = tmp.path().join("library");
        let bundle_dir = import_bundle_dir(&src, &output_dir).expect("dir import works");

        assert!(bundle_dir.join("image.png").exists());
        assert!(bundle_dir.join("model.glb").exists());
        assert!(bundle_dir.join("textures/base.png").exists());
        assert!(!bundle_dir.join(".DS_Store").exists());
        // Source untouched.
        assert!(src.join("image.png").exists());
    }

    #[test]
    fn test_import_bundle_dir_rejects_empty_and_invalid() {
        let tmp = tempfile::tempdir().unwrap();
        let empty = tmp.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        let output_dir = tmp.path().join("library");
        assert!(import_bundle_dir(&empty, &output_dir).is_err());

        let no_assets = tmp.path().join("no-assets");
        std::fs::create_dir_all(&no_assets).unwrap();
        std::fs::write(no_assets.join("readme.txt"), b"hi").unwrap();
        let err = import_bundle_dir(&no_assets, &output_dir).unwrap_err();
        assert!(err.contains("image (image.png) or model (model.glb)"));
    }

    #[test]
    fn test_detect_common_prefix() {
        // No prefix — files at root
        assert_eq!(
            detect_common_prefix(&["image.png".into(), "model.glb".into()]),
            None
        );

        // Common prefix
        assert_eq!(
            detect_common_prefix(&["bundle/image.png".into(), "bundle/model.glb".into()]),
            Some("bundle/".into())
        );

        // Mixed — one at root, one nested
        assert_eq!(
            detect_common_prefix(&["image.png".into(), "bundle/model.glb".into()]),
            None
        );

        // Different prefixes
        assert_eq!(
            detect_common_prefix(&["a/image.png".into(), "b/model.glb".into()]),
            None
        );

        // Empty
        assert_eq!(detect_common_prefix(&[]), None);
    }

    #[test]
    fn test_sha256_hex_known_value() {
        // SHA-256 of empty input is a well-known constant
        let hash = sha256_hex(b"");
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_sha256_hex_deterministic() {
        let data = b"hello world";
        assert_eq!(sha256_hex(data), sha256_hex(data));
    }

    #[test]
    fn test_verify_sha256_pass() {
        let data = b"test data";
        let hash = sha256_hex(data);
        assert!(verify_sha256(data, &hash).is_ok());
    }

    #[test]
    fn test_verify_sha256_fail() {
        let data = b"test data";
        let result = verify_sha256(
            data,
            "0000000000000000000000000000000000000000000000000000000000000000",
        );
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Integrity check failed"));
    }

    #[test]
    fn test_sha256_roundtrip_with_zip() {
        // Simulate the release workflow: create a zip, hash it, verify it
        let zip_data = create_test_zip(&[("image.png", b"fake-png"), ("model.glb", b"fake-glb")]);
        let hash = sha256_hex(&zip_data);

        // Verification should pass with the correct hash
        assert!(verify_sha256(&zip_data, &hash).is_ok());

        // Tampered data should fail
        let mut tampered = zip_data.clone();
        if let Some(byte) = tampered.last_mut() {
            *byte ^= 0xFF;
        }
        assert!(verify_sha256(&tampered, &hash).is_err());
    }

    /// Guards against docs drifting from the BundleMetadata struct.
    /// If someone adds a field to the docs' example JSON that doesn't exist on
    /// the struct (or vice versa), this test should fail so the mismatch is
    /// caught before it ships.
    #[test]
    fn test_docs_bundle_example_matches_struct() {
        // Mirrors the Version 1 example in docs/guides/BUNDLE_STRUCTURE.md.
        // Keep this in sync with that block.
        let doc_example = r#"{
            "version": 1,
            "name": "a cowboy ninja",
            "created_at": "2024-12-29T15:30:45Z",
            "config": {
                "prompt": "a cowboy ninja",
                "image_model": "fal-ai/nano-banana-2",
                "model_3d": "fal-ai/trellis-2",
                "export_fbx": true,
                "image_model_params": {
                    "guidance_scale": 4.5,
                    "num_inference_steps": 32
                },
                "model_3d_params": {
                    "topology": "quad",
                    "target_polycount": 50000
                }
            },
            "model_info": {
                "file_size": 2739808,
                "format": "GLB",
                "vertex_count": 27398,
                "triangle_count": 9132
            }
        }"#;

        let parsed: BundleMetadata = serde_json::from_str(doc_example)
            .expect("docs example JSON must deserialize into BundleMetadata");
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.name.as_deref(), Some("a cowboy ninja"));
        let cfg = parsed.config.clone().expect("config present");
        assert_eq!(cfg.image_model.as_deref(), Some("fal-ai/nano-banana-2"));
        assert_eq!(cfg.model_3d, "fal-ai/trellis-2");
        assert!(cfg.export_fbx);
        assert_eq!(cfg.image_model_params.len(), 2);
        assert_eq!(
            cfg.model_3d_params.get("topology").and_then(|v| v.as_str()),
            Some("quad")
        );

        // Re-serialize and confirm no phantom fields sneak in via round-trip.
        let round_tripped = serde_json::to_value(&parsed).expect("serialize");
        let top_level = round_tripped.as_object().expect("object");
        for forbidden in ["provider", "files", "generation_metadata"] {
            assert!(
                !top_level.contains_key(forbidden),
                "BundleMetadata must not serialize a `{}` field (see docs drift history)",
                forbidden
            );
        }
        let cfg_obj = top_level["config"].as_object().expect("config object");
        assert!(
            !cfg_obj.contains_key("provider"),
            "GenerationConfig must not serialize a `provider` field"
        );
        // A loaded v1 example must not grow v2 keys on re-serialize (empty
        // artifacts / missing pipeline are skip_serializing_if).
        assert!(!top_level.contains_key("artifacts"));
        assert!(!top_level.contains_key("pipeline"));
        assert!(!top_level.contains_key("primary"));
        assert!(!top_level.contains_key("category"));
    }

    #[test]
    fn test_v1_projects_full_v2_view() {
        let parsed: BundleMetadata = serde_json::from_str(
            r#"{
            "version": 1,
            "created_at": "2024-12-29T15:30:45Z",
            "config": {
                "prompt": "a crate",
                "image_model": "fal-ai/nano-banana-2",
                "model_3d": "fal-ai/trellis-2",
                "export_fbx": false
            }
        }"#,
        )
        .unwrap();
        let (artifacts, primary, category, pipeline) = parsed.v2_view();
        assert_eq!(primary.as_deref(), Some("model"));
        assert!(category.is_none());
        assert_eq!(artifacts.len(), 2);
        assert_eq!(pipeline.steps.len(), 2);
        assert!(parsed.artifacts.is_empty());
    }

    #[test]
    fn test_docs_bundle_v2_example_matches_struct() {
        // Mirrors the current (version 2) example in docs/guides/BUNDLE_STRUCTURE.md.
        let doc_example = r#"{
            "version": 2,
            "name": "a cowboy ninja",
            "created_at": "2024-12-29T15:30:45Z",
            "primary": "model",
            "artifacts": [
                {
                    "id": "image",
                    "role": "image",
                    "path": "image.png",
                    "mime": "image/png",
                    "produced_by": "image"
                },
                {
                    "id": "model",
                    "role": "model",
                    "path": "model.glb",
                    "mime": "model/gltf-binary",
                    "produced_by": "model",
                    "vertex_count": 27398,
                    "triangle_count": 9132
                }
            ],
            "pipeline": {
                "steps": [
                    {
                        "id": "image",
                        "kind": "model",
                        "provider": "fal.ai",
                        "model": "fal-ai/nano-banana-2",
                        "modality": "text_to_image",
                        "prompt": "a cowboy ninja",
                        "outputs": ["image"]
                    },
                    {
                        "id": "model",
                        "kind": "model",
                        "provider": "fal.ai",
                        "model": "fal-ai/trellis-2",
                        "modality": "image_to_3d",
                        "inputs": ["image"],
                        "outputs": ["model"]
                    }
                ]
            }
        }"#;

        let parsed: BundleMetadata = serde_json::from_str(doc_example)
            .expect("v2 docs example JSON must deserialize into BundleMetadata");
        assert_eq!(parsed.version, 2);
        assert_eq!(parsed.primary.as_deref(), Some("model"));
        assert!(parsed.category.is_none());
        assert_eq!(parsed.artifacts.len(), 2);
        let steps = &parsed.pipeline.as_ref().expect("pipeline").steps;
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].id(), "image");
        assert_eq!(steps[1].id(), "model");
        assert!(parsed.config.is_none());
        assert!(parsed.model_info.is_none());
        let view = parsed.generation_view();
        assert_eq!(view.prompt.as_deref(), Some("a cowboy ninja"));
        assert_eq!(view.image_model.as_deref(), Some("fal-ai/nano-banana-2"));
        assert_eq!(view.model_3d.as_deref(), Some("fal-ai/trellis-2"));
        assert_eq!(view.vertex_count, Some(27398));
    }

    #[test]
    fn for_generation_omits_v1_fields() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("image.png"), b"png").unwrap();
        std::fs::write(dir.path().join("model.glb"), b"glb").unwrap();
        let meta = BundleMetadata::for_generation(
            dir.path(),
            GenerationConfig {
                prompt: Some("a crate".into()),
                image_model: Some("fal-ai/nano-banana-2".into()),
                model_3d: "fal-ai/trellis-2".into(),
                ..GenerationConfig::default()
            },
            None,
            Some("fal.ai"),
            Some("fal.ai"),
        );
        let json = serde_json::to_value(&meta).unwrap();
        assert_eq!(json["version"], 2);
        assert!(json.get("config").is_none());
        assert!(json.get("model_info").is_none());
        assert!(json.get("category").is_none());
        assert!(json["artifacts"].as_array().unwrap().len() >= 2);
    }

    #[test]
    fn load_does_not_rewrite_v1_to_v2() {
        let dir = tempfile::tempdir().unwrap();
        let json = r#"{
            "version": 1,
            "created_at": "2024-12-29T15:30:45Z",
            "config": {
                "prompt": "a crate",
                "image_model": "fal-ai/nano-banana-2",
                "model_3d": "fal-ai/trellis-2",
                "export_fbx": false
            }
        }"#;
        std::fs::write(dir.path().join(BUNDLE_METADATA_FILE), json).unwrap();
        let loaded = BundleMetadata::load(dir.path()).unwrap().unwrap();
        assert_eq!(loaded.version, 1);
        assert!(loaded.artifacts.is_empty());
        let on_disk = std::fs::read_to_string(dir.path().join(BUNDLE_METADATA_FILE)).unwrap();
        let disk_val: serde_json::Value = serde_json::from_str(&on_disk).unwrap();
        assert_eq!(disk_val["version"], 1);
        assert!(disk_val.get("artifacts").is_none());
    }
}
