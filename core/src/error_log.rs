//! Structured error logging for debugging.
//!
//! Creates detailed error dumps tied to specific generations for:
//! - Debugging failed generations
//! - Sharing error reports
//! - Post-mortem analysis
//!
//! ## File Locations
//!
//! Error logs are stored in a `logs/` subdirectory:
//! - **Dev mode**: `.dev/logs/`
//! - **Release mode**: OS-specific config directory `logs/` subfolder
//!
//! Each error gets its own timestamped file: `error.YYYY-MM-DD_HHMMSS.json`

use crate::settings::is_dev_mode;
use crate::types::Stage;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Logs subdirectory name.
const LOGS_DIR: &str = "logs";

/// A structured error log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorLog {
    /// Unique error ID (timestamp-based).
    pub id: String,

    /// When the error occurred.
    pub timestamp: DateTime<Utc>,

    /// Associated generation ID (if applicable).
    pub generation_id: Option<String>,

    /// The stage where the error occurred.
    pub stage: Option<Stage>,

    /// Error category/type.
    pub error_type: ErrorType,

    /// Human-readable error message.
    pub message: String,

    /// Technical details (stack trace, API response, etc.).
    pub details: Option<String>,

    /// Request/response data for API errors.
    pub api_context: Option<ApiContext>,

    /// Environment information for debugging.
    pub environment: EnvironmentInfo,

    /// User's configuration at time of error.
    pub config_snapshot: Option<ConfigSnapshot>,
}

/// Type/category of error.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorType {
    /// API request failed.
    ApiError,
    /// Network/HTTP error.
    NetworkError,
    /// File I/O error.
    IoError,
    /// Invalid configuration or input.
    ValidationError,
    /// Model processing error.
    ProcessingError,
    /// External tool error (Blender, etc.).
    ToolError,
    /// Unknown/uncategorized error.
    Unknown,
}

/// API request/response context for debugging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiContext {
    /// API endpoint that was called.
    pub endpoint: Option<String>,

    /// HTTP method used.
    pub method: Option<String>,

    /// HTTP status code received.
    pub status_code: Option<u16>,

    /// Request body (sanitized - no API keys).
    pub request_body: Option<String>,

    /// Response body (truncated if large).
    pub response_body: Option<String>,

    /// Request ID from API (if available).
    pub request_id: Option<String>,
}

/// Environment information for debugging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentInfo {
    /// Application version.
    pub app_version: String,

    /// Operating system.
    pub os: String,

    /// OS version.
    pub os_version: Option<String>,

    /// Whether running in dev mode.
    pub dev_mode: bool,

    /// Blender availability.
    pub blender_available: bool,

    /// Blender version (if available).
    pub blender_version: Option<String>,
}

impl Default for EnvironmentInfo {
    fn default() -> Self {
        Self {
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            os: std::env::consts::OS.to_string(),
            os_version: get_os_version(),
            dev_mode: is_dev_mode(),
            blender_available: crate::convert::is_blender_available(),
            blender_version: get_blender_version(),
        }
    }
}

/// Get the OS version string.
fn get_os_version() -> Option<String> {
    let output = std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()?;
    if output.status.success() {
        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !version.is_empty() {
            return Some(version);
        }
    }

    // Fallback: uname -r (works on Linux/macOS)
    let output = std::process::Command::new("uname")
        .arg("-r")
        .output()
        .ok()?;
    if output.status.success() {
        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !version.is_empty() {
            return Some(version);
        }
    }
    None
}

/// Get the Blender version string (if Blender is available).
fn get_blender_version() -> Option<String> {
    let blender_cmd = crate::convert::find_blender()?;
    // Handle multi-part commands like "flatpak run org.blender.Blender"
    let parts: Vec<&str> = blender_cmd.split_whitespace().collect();
    let output = std::process::Command::new(parts[0])
        .args(&parts[1..])
        .arg("--version")
        .output()
        .ok()?;
    if output.status.success() {
        // First line is typically "Blender 4.2.0"
        let stdout = String::from_utf8_lossy(&output.stdout);
        let first_line = stdout.lines().next()?.trim().to_string();
        if !first_line.is_empty() {
            return Some(first_line);
        }
    }
    None
}

/// Snapshot of relevant configuration at time of error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSnapshot {
    /// Prompt being used.
    pub prompt: Option<String>,

    /// Image model selected.
    pub image_model: Option<String>,

    /// 3D model selected.
    pub model_3d: Option<String>,

    /// Whether FBX export was enabled.
    pub export_fbx: bool,

    /// Number of style references.
    pub style_ref_count: usize,
}

impl ErrorLog {
    /// Create a new error log entry.
    pub fn new(error_type: ErrorType, message: impl Into<String>) -> Self {
        Self {
            id: crate::config::generate_timestamp(),
            timestamp: Utc::now(),
            generation_id: None,
            stage: None,
            error_type,
            message: message.into(),
            details: None,
            api_context: None,
            environment: EnvironmentInfo::default(),
            config_snapshot: None,
        }
    }

    /// Create an error log from a structured ApiError.
    pub fn from_api_error(api_error: &crate::types::ApiError, stage: Option<Stage>) -> Self {
        use crate::types::ApiErrorKind;

        let error_type = match api_error.kind {
            ApiErrorKind::NetworkError => ErrorType::NetworkError,
            ApiErrorKind::ValidationError => ErrorType::ValidationError,
            ApiErrorKind::ModelError => ErrorType::ProcessingError,
            _ => ErrorType::ApiError,
        };

        Self {
            id: crate::config::generate_timestamp(),
            timestamp: Utc::now(),
            generation_id: None,
            stage,
            error_type,
            message: api_error.user_message.clone(),
            details: Some(api_error.raw_message.clone()),
            api_context: Some(ApiContext {
                endpoint: api_error.endpoint.clone(),
                method: api_error.method.clone(),
                status_code: api_error.status_code,
                request_body: None,
                response_body: Some(api_error.raw_message.clone()),
                request_id: None,
            }),
            environment: EnvironmentInfo::default(),
            config_snapshot: None,
        }
    }

    /// Set the associated generation ID.
    pub fn with_generation(mut self, generation_id: impl Into<String>) -> Self {
        self.generation_id = Some(generation_id.into());
        self
    }

    /// Set the stage where the error occurred.
    pub fn with_stage(mut self, stage: Stage) -> Self {
        self.stage = Some(stage);
        self
    }

    /// Add technical details.
    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    /// Add API context.
    pub fn with_api_context(mut self, context: ApiContext) -> Self {
        self.api_context = Some(context);
        self
    }

    /// Add configuration snapshot.
    pub fn with_config(mut self, config: ConfigSnapshot) -> Self {
        self.config_snapshot = Some(config);
        self
    }

    /// Save the error log to a file.
    ///
    /// Returns the path to the saved log file.
    pub fn save(&self) -> std::io::Result<PathBuf> {
        let logs_dir = logs_dir_path();
        std::fs::create_dir_all(&logs_dir)?;

        let filename = format!("error.{}.json", self.id);
        let path = logs_dir.join(filename);

        let contents = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;

        std::fs::write(&path, contents)?;

        // Restrict to owner-only on Unix (error logs may contain API response details)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }

        Ok(path)
    }

    /// Load an error log from a file.
    pub fn load(path: &std::path::Path) -> std::io::Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        serde_json::from_str(&contents).map_err(std::io::Error::other)
    }

    /// Generate a shareable summary of this error.
    pub fn summary(&self) -> String {
        let mut lines = vec![
            format!("Error ID: {}", self.id),
            format!("Time: {}", self.timestamp.format("%Y-%m-%d %H:%M:%S UTC")),
            format!("Type: {:?}", self.error_type),
            format!("Message: {}", self.message),
        ];

        if let Some(ref stage) = self.stage {
            lines.push(format!("Stage: {}", stage));
        }

        if let Some(ref gen_id) = self.generation_id {
            lines.push(format!("Generation: {}", gen_id));
        }

        if let Some(ref details) = self.details {
            lines.push(format!("\nDetails:\n{}", details));
        }

        lines.join("\n")
    }
}

/// Get the path to the logs directory.
pub fn logs_dir_path() -> PathBuf {
    if is_dev_mode() {
        PathBuf::from(crate::constants::files::dev_dirs::LOGS)
    } else {
        crate::settings::config_dir().join(LOGS_DIR)
    }
}

/// Clean up old application log files in the default logs directory.
///
/// `tracing-appender` daily rolling creates files like `app.log.2026-02-21`.
/// This removes any older than `keep_days`.
pub fn cleanup_old_app_logs(keep_days: usize) -> std::io::Result<usize> {
    cleanup_old_app_logs_in(&logs_dir_path(), keep_days)
}

/// Clean up old application log files in the given directory.
pub fn cleanup_old_app_logs_in(
    logs_dir: &std::path::Path,
    keep_days: usize,
) -> std::io::Result<usize> {
    if !logs_dir.exists() {
        return Ok(0);
    }

    let cutoff = chrono::Utc::now() - chrono::Duration::days(keep_days as i64);
    let cutoff_str = cutoff.format("%Y-%m-%d").to_string();

    let mut deleted = 0;
    for entry in std::fs::read_dir(logs_dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        // Match files like "app.log.2026-02-14"
        if let Some(date_suffix) = name.strip_prefix("app.log.")
            && date_suffix < cutoff_str.as_str()
            && std::fs::remove_file(entry.path()).is_ok()
        {
            deleted += 1;
        }
    }

    Ok(deleted)
}

/// Initialize tracing with dual output (console + rolling log file) and panic hook.
///
/// Returns a guard that must be held alive for the duration of the program
/// to ensure log flushing. The guard is returned as a boxed `dyn Send` so callers
/// don't need to depend on `tracing_appender` directly.
///
/// When `quiet_console` is true, the stderr layer is filtered to WARN+ so
/// interactive prompts (e.g. `auth set`) aren't buried in startup INFO logs.
/// The file layer still captures INFO for debugging.
pub fn init_tracing(quiet_console: bool) -> Box<dyn Send> {
    use tracing_subscriber::prelude::*;

    let env_filter = tracing_subscriber::EnvFilter::from_default_env()
        .add_directive(tracing::Level::INFO.into());

    let logs_dir = logs_dir_path();
    std::fs::create_dir_all(&logs_dir).ok();
    let file_appender = tracing_appender::rolling::daily(&logs_dir, "app.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_filter(if quiet_console {
            tracing_subscriber::filter::LevelFilter::WARN
        } else {
            tracing_subscriber::filter::LevelFilter::TRACE
        });

    tracing_subscriber::registry()
        .with(env_filter)
        .with(stderr_layer)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false),
        )
        .init();

    // Install panic hook to log crashes
    std::panic::set_hook(Box::new(|info| {
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic payload".to_string()
        };

        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()));

        let backtrace = std::backtrace::Backtrace::force_capture();

        tracing::error!(
            panic.payload = %payload,
            panic.location = ?location,
            "PANIC: {}\nLocation: {}\nBacktrace:\n{}",
            payload,
            location.as_deref().unwrap_or("unknown"),
            backtrace,
        );

        eprintln!("\nPANIC: {}", payload);
        if let Some(loc) = info.location() {
            eprintln!("Location: {}:{}:{}", loc.file(), loc.line(), loc.column());
        }
        eprintln!("Backtrace:\n{}", backtrace);
    }));

    // Clean up old log files (keep 7 days)
    if let Err(e) = cleanup_old_app_logs(7) {
        tracing::warn!("Failed to clean up old log files: {}", e);
    }

    Box::new(guard)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_log_creation() {
        let log = ErrorLog::new(ErrorType::ApiError, "Test error message")
            .with_generation("20241229_153045")
            .with_stage(Stage::ImageGeneration);

        assert_eq!(log.message, "Test error message");
        assert_eq!(log.generation_id.as_deref(), Some("20241229_153045"));
        assert!(matches!(log.error_type, ErrorType::ApiError));
    }

    #[test]
    fn test_error_log_serialization() {
        let log = ErrorLog::new(ErrorType::NetworkError, "Connection failed");

        let json = serde_json::to_string(&log).unwrap();
        let loaded: ErrorLog = serde_json::from_str(&json).unwrap();

        assert_eq!(log.id, loaded.id);
        assert_eq!(log.message, loaded.message);
    }

    #[test]
    fn test_error_summary() {
        let log = ErrorLog::new(ErrorType::IoError, "File not found")
            .with_stage(Stage::FbxConversion)
            .with_details("Could not locate model.glb");

        let summary = log.summary();
        assert!(summary.contains("File not found"));
        assert!(summary.contains("FBX Conversion"));
    }

    #[test]
    fn test_cleanup_old_app_logs_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let result = cleanup_old_app_logs_in(tmp.path(), 7);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[test]
    fn test_cleanup_old_app_logs_nonexistent_dir() {
        let result = cleanup_old_app_logs_in(std::path::Path::new("/nonexistent"), 7);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[test]
    fn test_cleanup_old_app_logs_removes_old_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        // Create old log files
        std::fs::write(dir.join("app.log.2020-01-01"), "old").unwrap();
        std::fs::write(dir.join("app.log.2020-01-02"), "old").unwrap();

        // Create recent log file (today)
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        std::fs::write(dir.join(format!("app.log.{}", today)), "recent").unwrap();

        // Create current log file (no date suffix)
        std::fs::write(dir.join("app.log"), "current").unwrap();

        // Create error JSON (should not be touched)
        std::fs::write(dir.join("error_20200101_120000.json"), "{}").unwrap();

        let deleted = cleanup_old_app_logs_in(dir, 7).unwrap();
        assert_eq!(deleted, 2);

        // Verify old files are gone
        assert!(!dir.join("app.log.2020-01-01").exists());
        assert!(!dir.join("app.log.2020-01-02").exists());

        // Verify recent/current files remain
        assert!(dir.join(format!("app.log.{}", today)).exists());
        assert!(dir.join("app.log").exists());
        assert!(dir.join("error_20200101_120000.json").exists());
    }

    #[test]
    fn test_from_api_error_preserves_endpoint_and_method() {
        use crate::types::{ApiError, ApiProvider};

        let provider = ApiProvider::new("fal.ai");
        let mut api_err = ApiError::from_response(provider, 500, "Internal server error", None);
        api_err.endpoint = Some("https://queue.fal.run/model/requests/abc/status".to_string());
        api_err.method = Some("GET".to_string());

        let log = ErrorLog::from_api_error(&api_err, Some(Stage::ImageGeneration));

        let ctx = log.api_context.unwrap();
        assert_eq!(
            ctx.endpoint.as_deref(),
            Some("https://queue.fal.run/model/requests/abc/status")
        );
        assert_eq!(ctx.method.as_deref(), Some("GET"));
        assert_eq!(ctx.status_code, Some(500));
        assert_eq!(ctx.response_body.as_deref(), Some("Internal server error"));
    }

    #[test]
    fn test_from_api_error_model_error_no_status() {
        use crate::types::{ApiError, ApiProvider};

        let provider = ApiProvider::new("fal.ai");
        let mut api_err = ApiError::from_model_error(provider, "GPU out of memory");
        api_err.endpoint = Some("https://queue.fal.run/model/requests/xyz/status".to_string());
        api_err.method = Some("GET".to_string());

        let log = ErrorLog::from_api_error(&api_err, Some(Stage::Model3DGeneration));

        assert!(matches!(log.error_type, ErrorType::ProcessingError));
        assert!(matches!(log.stage, Some(Stage::Model3DGeneration)));

        let ctx = log.api_context.unwrap();
        assert_eq!(
            ctx.endpoint.as_deref(),
            Some("https://queue.fal.run/model/requests/xyz/status")
        );
        assert_eq!(ctx.method.as_deref(), Some("GET"));
        assert_eq!(ctx.status_code, None);
    }

    #[test]
    fn test_error_log_save_with_config_snapshot() {
        let tmp = tempfile::tempdir().unwrap();
        let logs_dir = tmp.path().join("logs");
        std::fs::create_dir_all(&logs_dir).unwrap();

        let log = ErrorLog::new(ErrorType::ApiError, "Generation failed: GPU out of memory")
            .with_stage(Stage::ImageGeneration)
            .with_details("Full error chain here")
            .with_config(ConfigSnapshot {
                prompt: Some("a robot knight".to_string()),
                image_model: Some("fal-ai/nano-banana".to_string()),
                model_3d: Some("fal-ai/trellis-2".to_string()),
                export_fbx: true,
                style_ref_count: 0,
            });

        // Save to the temp directory (override logs_dir by writing directly)
        let path = logs_dir.join(format!("error_{}.json", log.id));
        let json = serde_json::to_string_pretty(&log).unwrap();
        std::fs::write(&path, &json).unwrap();

        // Load and verify
        assert!(path.exists());
        let loaded: ErrorLog =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(loaded.message, "Generation failed: GPU out of memory");
        assert!(matches!(loaded.stage, Some(Stage::ImageGeneration)));
        assert_eq!(loaded.details.as_deref(), Some("Full error chain here"));

        let config = loaded.config_snapshot.unwrap();
        assert_eq!(config.prompt.as_deref(), Some("a robot knight"));
        assert_eq!(config.image_model.as_deref(), Some("fal-ai/nano-banana"));
        assert_eq!(config.model_3d.as_deref(), Some("fal-ai/trellis-2"));
        assert!(config.export_fbx);
    }
}
