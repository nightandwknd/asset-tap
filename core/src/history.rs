//! Generation history tracking.
//!
//! Maintains a log of all pipeline executions for:
//! - Historical reference and analytics
//! - Reproducing previous generations
//! - Debugging and error correlation
//!
//! ## File Locations
//!
//! - **Dev mode**: `./.dev/history.json`
//! - **Release mode**: OS-specific config directory alongside `settings.json`
//!
//! ## Structure
//!
//! Each generation gets a unique ID (timestamp-based) and records:
//! - Configuration used (prompt, models, options)
//! - Outcome (success/failure)
//! - Timing information
//! - File paths to outputs
//! - Any errors encountered

use crate::constants::files::config as config_files;
use crate::constants::files::dev_dirs;
use crate::constants::validation::MAX_HISTORY_RECORDS;
use crate::pipeline::PipelineConfig;
use crate::settings::is_dev_mode;
use crate::types::PipelineOutput;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

/// History filename.
const HISTORY_FILE: &str = config_files::HISTORY;

/// A single generation record in history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationRecord {
    /// Unique identifier (timestamp-based, e.g., "20241229_153045").
    pub id: String,

    /// When the generation started.
    pub started_at: DateTime<Utc>,

    /// When the generation completed (None if in-progress or crashed).
    pub completed_at: Option<DateTime<Utc>>,

    /// Duration in milliseconds (calculated on completion).
    pub duration_ms: Option<u64>,

    /// The configuration used for this generation.
    pub config: GenerationConfig,

    /// The outcome of the generation.
    pub status: GenerationStatus,

    /// Output paths (populated on success).
    pub output: Option<GenerationOutput>,

    /// Error information (populated on failure).
    pub error: Option<ErrorInfo>,
}

/// Serializable version of pipeline configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GenerationConfig {
    /// Text prompt used (after template expansion, if any).
    pub prompt: Option<String>,

    /// Original user input before template expansion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_prompt: Option<String>,

    /// Template name used for prompt (if any).
    pub template: Option<String>,

    /// Existing image URL/path (if skipping generation).
    pub existing_image: Option<String>,

    /// Image generation model.
    pub image_model: Option<String>,

    /// 3D generation model.
    pub model_3d: String,

    /// Whether FBX export was enabled.
    pub export_fbx: bool,

    /// User-tuned parameter overrides applied to the image model (e.g.
    /// `guidance_scale`, `num_inference_steps`). Empty when no overrides were
    /// set; serialized only when non-empty so older bundles stay clean.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub image_model_params: HashMap<String, serde_json::Value>,

    /// User-tuned parameter overrides applied to the 3D model (e.g.
    /// `topology`, `target_polycount`, `enable_pbr`).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub model_3d_params: HashMap<String, serde_json::Value>,
}

impl From<&PipelineConfig> for GenerationConfig {
    fn from(config: &PipelineConfig) -> Self {
        Self {
            prompt: config.prompt.clone(),
            user_prompt: config.user_prompt.clone(),
            template: config.template.clone(),
            existing_image: config.image_url.as_deref().map(sanitize_image_reference),
            image_model: config.image_model.clone(),
            model_3d: config.model_3d.clone(),
            export_fbx: config.export_fbx,
            image_model_params: config.image_model_params.clone(),
            model_3d_params: config.model_3d_params.clone(),
        }
    }
}

/// Sanitize an image reference for serialization into shareable bundle metadata.
///
/// Local filesystem paths leak the user's directory layout (PII when bundles are
/// shared). Strip them to just the filename. Pass URLs and data URIs through
/// unchanged — they're already non-PII and may be useful for reference.
fn sanitize_image_reference(reference: &str) -> String {
    let lower = reference.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("data:") {
        return reference.to_string();
    }
    std::path::Path::new(reference)
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| reference.to_string())
}

/// Status of a generation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GenerationStatus {
    /// Generation is currently running.
    InProgress,
    /// Generation completed successfully.
    Completed,
    /// Generation failed with an error.
    Failed,
    /// Generation was cancelled by the user.
    Cancelled,
    /// Generation was interrupted (app crash/close).
    Interrupted,
}

/// Output paths from a successful generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationOutput {
    /// Output directory for this generation.
    pub output_dir: Option<PathBuf>,

    /// Generated/input image path.
    pub image_path: Option<PathBuf>,

    /// 3D model path (GLB).
    pub model_path: Option<PathBuf>,

    /// FBX export path (if conversion succeeded).
    pub fbx_path: Option<PathBuf>,

    /// Textures directory (if textures were extracted).
    pub textures_dir: Option<PathBuf>,
}

impl From<&PipelineOutput> for GenerationOutput {
    fn from(output: &PipelineOutput) -> Self {
        Self {
            output_dir: output.output_dir.clone(),
            image_path: output.image_path.clone(),
            model_path: output.model_path.clone(),
            fbx_path: output.fbx_path.clone(),
            textures_dir: output.textures_dir.clone(),
        }
    }
}

/// Error information for failed generations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorInfo {
    /// Error message.
    pub message: String,

    /// Stage where the error occurred.
    pub stage: Option<String>,

    /// Stack trace or additional context.
    pub details: Option<String>,

    /// Associated error log file (if written).
    pub log_file: Option<PathBuf>,

    /// Partial output (if some stages completed before failure).
    /// For example, if image generation succeeded but 3D generation failed,
    /// this contains the image path for recovery.
    pub partial_output: Option<GenerationOutput>,
}

/// Generation history manager.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GenerationHistory {
    /// All generation records, newest first.
    pub records: VecDeque<GenerationRecord>,

    /// Total number of generations ever run (for stats).
    pub total_generations: u64,

    /// Total successful generations.
    pub successful_generations: u64,

    /// Total failed generations.
    pub failed_generations: u64,
}

impl GenerationHistory {
    /// Load history from the history file.
    pub fn load() -> Self {
        let path = history_file_path();

        if !path.exists() {
            return Self::default();
        }

        match std::fs::read_to_string(&path) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Save history to the history file.
    pub fn save(&self) -> std::io::Result<()> {
        let path = history_file_path();

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let contents = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;

        std::fs::write(&path, contents)
    }

    /// Start a new generation and add it to history.
    ///
    /// Returns the generation ID for tracking.
    pub fn start_generation(&mut self, config: &PipelineConfig) -> String {
        let id = crate::config::generate_timestamp();

        let record = GenerationRecord {
            id: id.clone(),
            started_at: Utc::now(),
            completed_at: None,
            duration_ms: None,
            config: GenerationConfig::from(config),
            status: GenerationStatus::InProgress,
            output: None,
            error: None,
        };

        // Add to front (newest first)
        self.records.push_front(record);
        self.total_generations += 1;

        // Trim old records if over limit
        while self.records.len() > MAX_HISTORY_RECORDS {
            self.records.pop_back();
        }

        let _ = self.save();
        id
    }

    /// Mark a generation as completed successfully.
    pub fn complete_generation(&mut self, id: &str, output: &PipelineOutput) {
        if let Some(record) = self.find_record_mut(id) {
            let now = Utc::now();
            record.completed_at = Some(now);
            record.duration_ms = Some((now - record.started_at).num_milliseconds().max(0) as u64);
            record.status = GenerationStatus::Completed;
            record.output = Some(GenerationOutput::from(output));
            self.successful_generations += 1;
            let _ = self.save();
        }
    }

    /// Mark a generation as failed.
    pub fn fail_generation(&mut self, id: &str, error: ErrorInfo) {
        if let Some(record) = self.find_record_mut(id) {
            let now = Utc::now();
            record.completed_at = Some(now);
            record.duration_ms = Some((now - record.started_at).num_milliseconds().max(0) as u64);
            record.status = GenerationStatus::Failed;
            record.error = Some(error);
            self.failed_generations += 1;
            let _ = self.save();
        }
    }

    /// Mark a generation as cancelled.
    pub fn cancel_generation(&mut self, id: &str) {
        if let Some(record) = self.find_record_mut(id) {
            record.completed_at = Some(Utc::now());
            record.status = GenerationStatus::Cancelled;
            let _ = self.save();
        }
    }

    /// Mark any in-progress generations as interrupted.
    ///
    /// Called on startup to clean up after crashes.
    pub fn mark_interrupted(&mut self) {
        let mut changed = false;
        for record in &mut self.records {
            if record.status == GenerationStatus::InProgress {
                record.status = GenerationStatus::Interrupted;
                changed = true;
            }
        }
        if changed {
            let _ = self.save();
        }
    }

    /// Get a record by ID.
    pub fn get_record(&self, id: &str) -> Option<&GenerationRecord> {
        self.records.iter().find(|r| r.id == id)
    }

    /// Find a mutable record by ID.
    fn find_record_mut(&mut self, id: &str) -> Option<&mut GenerationRecord> {
        self.records.iter_mut().find(|r| r.id == id)
    }

    /// Get the most recent N records.
    pub fn recent(&self, count: usize) -> impl Iterator<Item = &GenerationRecord> {
        self.records.iter().take(count)
    }

    /// Get records matching a status filter.
    pub fn filter_by_status(&self, status: GenerationStatus) -> Vec<&GenerationRecord> {
        self.records.iter().filter(|r| r.status == status).collect()
    }

    /// Search records by prompt text.
    pub fn search(&self, query: &str) -> Vec<&GenerationRecord> {
        let query_lower = query.to_lowercase();
        self.records
            .iter()
            .filter(|r| {
                r.config
                    .prompt
                    .as_ref()
                    .map(|p| p.to_lowercase().contains(&query_lower))
                    .unwrap_or(false)
            })
            .collect()
    }

    /// Get statistics about generation history.
    pub fn stats(&self) -> HistoryStats {
        let avg_duration: Option<f64> = {
            let durations: Vec<u64> = self.records.iter().filter_map(|r| r.duration_ms).collect();
            if durations.is_empty() {
                None
            } else {
                Some(durations.iter().sum::<u64>() as f64 / durations.len() as f64)
            }
        };

        HistoryStats {
            total_generations: self.total_generations,
            successful_generations: self.successful_generations,
            failed_generations: self.failed_generations,
            average_duration_ms: avg_duration,
        }
    }
}

/// Statistics about generation history.
#[derive(Debug, Clone)]
pub struct HistoryStats {
    pub total_generations: u64,
    pub successful_generations: u64,
    pub failed_generations: u64,
    pub average_duration_ms: Option<f64>,
}

/// Get the path to the history file.
pub fn history_file_path() -> PathBuf {
    if is_dev_mode() {
        PathBuf::from(dev_dirs::ROOT).join(HISTORY_FILE)
    } else {
        crate::settings::config_dir().join(HISTORY_FILE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_history() {
        let history = GenerationHistory::default();
        assert!(history.records.is_empty());
        assert_eq!(history.total_generations, 0);
        assert_eq!(history.successful_generations, 0);
        assert_eq!(history.failed_generations, 0);
    }

    #[test]
    fn test_record_serialization() {
        let record = GenerationRecord {
            id: "20241229_153045".to_string(),
            started_at: Utc::now(),
            completed_at: None,
            duration_ms: None,
            config: GenerationConfig {
                prompt: Some("a cowboy ninja with a leather duster, bandana mask, and dual katanas on the back".to_string()),
                user_prompt: None,
                template: None,
                existing_image: None,
                image_model: Some("nano-banana".to_string()),
                model_3d: "trellis-2".to_string(),
                export_fbx: true,
                            image_model_params: std::collections::HashMap::new(),
                model_3d_params: std::collections::HashMap::new(),
            },
            status: GenerationStatus::InProgress,
            output: None,
            error: None,
        };

        let json = serde_json::to_string(&record).unwrap();
        let loaded: GenerationRecord = serde_json::from_str(&json).unwrap();

        assert_eq!(record.id, loaded.id);
        assert_eq!(record.config.prompt, loaded.config.prompt);
    }

    #[test]
    fn test_sanitize_image_reference_strips_absolute_paths() {
        // Absolute paths are stripped to filename to avoid leaking PII when
        // bundles are shared.
        assert_eq!(
            sanitize_image_reference("/Users/alice/Documents/secret/image.png"),
            "image.png"
        );
        assert_eq!(
            sanitize_image_reference("/home/bob/projects/work/photo.jpg"),
            "photo.jpg"
        );
        // Windows-style paths: Path::file_name only splits on the platform's
        // separator, so we don't assert on backslash splitting here. The PII
        // case we care about — POSIX absolute paths from shared bundles — is
        // covered above.

        // URLs and data URIs pass through unchanged.
        assert_eq!(
            sanitize_image_reference("https://example.com/image.png"),
            "https://example.com/image.png"
        );
        assert_eq!(
            sanitize_image_reference("HTTP://example.com/x.jpg"),
            "HTTP://example.com/x.jpg"
        );
        assert_eq!(
            sanitize_image_reference("data:image/png;base64,iVBOR..."),
            "data:image/png;base64,iVBOR..."
        );

        // Already-relative paths get reduced to their filename too — consistent
        // and still non-leaky.
        assert_eq!(sanitize_image_reference("photo.png"), "photo.png");
        assert_eq!(sanitize_image_reference("./local/photo.png"), "photo.png");
    }

    #[test]
    fn test_generation_config_from_pipeline_sanitizes_existing_image() {
        let config = PipelineConfig::new()
            .with_prompt("test")
            .with_existing_image("/Users/alice/secret-project/input.png")
            .with_3d_model("trellis-2");
        let g = GenerationConfig::from(&config);
        assert_eq!(g.existing_image.as_deref(), Some("input.png"));
    }

    #[test]
    fn test_generation_config_from_pipeline_config() {
        let mut pipeline_config = PipelineConfig::new()
            .with_prompt("a robot")
            .with_template("character")
            .with_3d_model("trellis-2")
            .with_image_model("nano-banana");
        pipeline_config.export_fbx = true;

        let gen_config = GenerationConfig::from(&pipeline_config);

        assert_eq!(gen_config.prompt, Some("a robot".to_string()));
        assert_eq!(gen_config.template, Some("character".to_string()));
        assert_eq!(gen_config.model_3d, "trellis-2");
        assert_eq!(gen_config.image_model, Some("nano-banana".to_string()));
        assert!(gen_config.export_fbx);
    }

    #[test]
    fn test_generation_output_from_pipeline_output() {
        let mut pipeline_output = PipelineOutput::new();
        pipeline_output.output_dir = Some(PathBuf::from("/output/20241229"));
        pipeline_output.image_path = Some(PathBuf::from("/output/20241229/image.png"));
        pipeline_output.model_path = Some(PathBuf::from("/output/20241229/model.glb"));
        pipeline_output.fbx_path = Some(PathBuf::from("/output/20241229/model.fbx"));
        pipeline_output.textures_dir = Some(PathBuf::from("/output/20241229/textures"));

        let gen_output = GenerationOutput::from(&pipeline_output);

        assert_eq!(gen_output.output_dir, pipeline_output.output_dir);
        assert_eq!(gen_output.image_path, pipeline_output.image_path);
        assert_eq!(gen_output.model_path, pipeline_output.model_path);
        assert_eq!(gen_output.fbx_path, pipeline_output.fbx_path);
        assert_eq!(gen_output.textures_dir, pipeline_output.textures_dir);
    }

    #[test]
    fn test_generation_status_serialization() {
        assert_eq!(
            serde_json::to_string(&GenerationStatus::InProgress).unwrap(),
            "\"in_progress\""
        );
        assert_eq!(
            serde_json::to_string(&GenerationStatus::Completed).unwrap(),
            "\"completed\""
        );
        assert_eq!(
            serde_json::to_string(&GenerationStatus::Failed).unwrap(),
            "\"failed\""
        );
        assert_eq!(
            serde_json::to_string(&GenerationStatus::Cancelled).unwrap(),
            "\"cancelled\""
        );
        assert_eq!(
            serde_json::to_string(&GenerationStatus::Interrupted).unwrap(),
            "\"interrupted\""
        );
    }

    #[test]
    fn test_history_stats_empty() {
        let history = GenerationHistory::default();
        let stats = history.stats();

        assert_eq!(stats.total_generations, 0);
        assert_eq!(stats.successful_generations, 0);
        assert_eq!(stats.failed_generations, 0);
        assert!(stats.average_duration_ms.is_none());
    }

    #[test]
    fn test_history_stats_with_records() {
        let mut history = GenerationHistory {
            total_generations: 3,
            successful_generations: 2,
            failed_generations: 1,
            ..Default::default()
        };

        // Add records with costs and durations
        history.records.push_back(GenerationRecord {
            id: "1".to_string(),
            started_at: Utc::now(),
            completed_at: Some(Utc::now()),
            duration_ms: Some(1000),
            config: GenerationConfig {
                prompt: None,
                user_prompt: None,
                template: None,
                existing_image: None,
                image_model: None,
                model_3d: "trellis-2".to_string(),
                export_fbx: false,
                image_model_params: std::collections::HashMap::new(),
                model_3d_params: std::collections::HashMap::new(),
            },
            status: GenerationStatus::Completed,
            output: None,
            error: None,
        });

        history.records.push_back(GenerationRecord {
            id: "2".to_string(),
            started_at: Utc::now(),
            completed_at: Some(Utc::now()),
            duration_ms: Some(2000),
            config: GenerationConfig {
                prompt: None,
                user_prompt: None,
                template: None,
                existing_image: None,
                image_model: None,
                model_3d: "trellis-2".to_string(),
                export_fbx: false,
                image_model_params: std::collections::HashMap::new(),
                model_3d_params: std::collections::HashMap::new(),
            },
            status: GenerationStatus::Completed,
            output: None,
            error: None,
        });

        let stats = history.stats();
        assert_eq!(stats.total_generations, 3);
        assert_eq!(stats.average_duration_ms, Some(1500.0));
    }

    #[test]
    fn test_history_filter_by_status() {
        let mut history = GenerationHistory::default();

        let make_record = |id: &str, status: GenerationStatus| GenerationRecord {
            id: id.to_string(),
            started_at: Utc::now(),
            completed_at: None,
            duration_ms: None,
            config: GenerationConfig {
                prompt: None,
                user_prompt: None,
                template: None,
                existing_image: None,
                image_model: None,
                model_3d: "trellis-2".to_string(),
                export_fbx: false,
                image_model_params: std::collections::HashMap::new(),
                model_3d_params: std::collections::HashMap::new(),
            },
            status,
            output: None,
            error: None,
        };

        history
            .records
            .push_back(make_record("1", GenerationStatus::Completed));
        history
            .records
            .push_back(make_record("2", GenerationStatus::Failed));
        history
            .records
            .push_back(make_record("3", GenerationStatus::Completed));

        let completed = history.filter_by_status(GenerationStatus::Completed);
        assert_eq!(completed.len(), 2);

        let failed = history.filter_by_status(GenerationStatus::Failed);
        assert_eq!(failed.len(), 1);
    }

    #[test]
    fn test_history_search() {
        let mut history = GenerationHistory::default();

        let make_record = |id: &str, prompt: Option<&str>| GenerationRecord {
            id: id.to_string(),
            started_at: Utc::now(),
            completed_at: None,
            duration_ms: None,
            config: GenerationConfig {
                prompt: prompt.map(String::from),
                user_prompt: None,
                template: None,
                existing_image: None,
                image_model: None,
                model_3d: "trellis-2".to_string(),
                export_fbx: false,
                image_model_params: std::collections::HashMap::new(),
                model_3d_params: std::collections::HashMap::new(),
            },
            status: GenerationStatus::Completed,
            output: None,
            error: None,
        };

        history.records.push_back(make_record(
            "1",
            Some(
                "a cowboy ninja with a leather duster, bandana mask, and dual katanas on the back",
            ),
        ));
        history
            .records
            .push_back(make_record("2", Some("a scary monster")));
        history
            .records
            .push_back(make_record("3", Some("another ROBOT character")));
        history.records.push_back(make_record("4", None));

        // Case-insensitive search (only record 3 contains "robot")
        let results = history.search("robot");
        assert_eq!(results.len(), 1);

        let results = history.search("MONSTER");
        assert_eq!(results.len(), 1);

        let results = history.search("dragon");
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_history_recent() {
        let mut history = GenerationHistory::default();

        let make_record = |id: &str| GenerationRecord {
            id: id.to_string(),
            started_at: Utc::now(),
            completed_at: None,
            duration_ms: None,
            config: GenerationConfig {
                prompt: None,
                user_prompt: None,
                template: None,
                existing_image: None,
                image_model: None,
                model_3d: "trellis-2".to_string(),
                export_fbx: false,
                image_model_params: std::collections::HashMap::new(),
                model_3d_params: std::collections::HashMap::new(),
            },
            status: GenerationStatus::Completed,
            output: None,
            error: None,
        };

        history.records.push_front(make_record("3"));
        history.records.push_front(make_record("2"));
        history.records.push_front(make_record("1"));

        let recent: Vec<_> = history.recent(2).collect();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].id, "1");
        assert_eq!(recent[1].id, "2");
    }

    #[test]
    fn test_history_get_record() {
        let mut history = GenerationHistory::default();

        history.records.push_back(GenerationRecord {
            id: "test123".to_string(),
            started_at: Utc::now(),
            completed_at: None,
            duration_ms: None,
            config: GenerationConfig {
                prompt: Some("test prompt".to_string()),
                user_prompt: None,
                template: None,
                existing_image: None,
                image_model: None,
                model_3d: "trellis-2".to_string(),
                export_fbx: false,
                image_model_params: std::collections::HashMap::new(),
                model_3d_params: std::collections::HashMap::new(),
            },
            status: GenerationStatus::InProgress,
            output: None,
            error: None,
        });

        assert!(history.get_record("test123").is_some());
        assert!(history.get_record("nonexistent").is_none());
    }

    #[test]
    fn test_error_info_serialization() {
        let error = ErrorInfo {
            message: "Failed to generate model".to_string(),
            stage: Some("3D Generation".to_string()),
            details: Some("OOM error".to_string()),
            log_file: Some(PathBuf::from("/logs/error.json")),
            partial_output: Some(GenerationOutput {
                output_dir: Some(PathBuf::from("/output/20241229")),
                image_path: Some(PathBuf::from("/output/20241229/image.png")),
                model_path: None,
                fbx_path: None,
                textures_dir: None,
            }),
        };

        let json = serde_json::to_string(&error).unwrap();
        let loaded: ErrorInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(error.message, loaded.message);
        assert_eq!(error.stage, loaded.stage);
        assert!(loaded.partial_output.is_some());
    }
}
