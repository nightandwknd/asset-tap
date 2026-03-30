//! Core types, errors, and result definitions.
//!
//! This module defines the fundamental types used throughout the Asset Tap:
//! - [`Error`] and [`Result<T>`] - Error handling
//! - [`Progress`] and [`Stage`] - Pipeline progress tracking
//! - [`PipelineOutput`] - Pipeline execution results
//! - [`ApiError`] and [`ApiErrorKind`] - Structured API error handling
//!
//! # See Also
//!
//! - [`pipeline`](crate::pipeline) - Pipeline execution
//! - [`providers`](crate::providers) - Provider system

use crate::constants::errors::{self, patterns};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Core result type for the Asset Tap.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur during pipeline execution.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Structured API error with provider context
    #[error("{0}")]
    ApiError(Box<ApiError>),

    /// Generic API error string.
    #[error("API error: {0}")]
    Api(String),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Missing API key: {0}")]
    MissingApiKey(String),

    #[error("Invalid model: {0}")]
    InvalidModel(String),

    #[error("Pipeline error: {0}")]
    Pipeline(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("File not found: {0}")]
    FileNotFound(PathBuf),

    #[error("Configuration error: {0}")]
    Config(String),
}

impl From<ApiError> for Error {
    fn from(err: ApiError) -> Self {
        Error::ApiError(Box::new(err))
    }
}

/// Structured API error with provider-specific context.
#[derive(Debug, Clone)]
pub struct ApiError {
    /// The API provider that returned this error
    pub provider: ApiProvider,
    /// The type of error
    pub kind: ApiErrorKind,
    /// HTTP status code (if available)
    pub status_code: Option<u16>,
    /// Raw error message from the API
    pub raw_message: String,
    /// User-friendly error message
    pub user_message: String,
    /// Suggested action to resolve the error
    pub action: Option<String>,
    /// Whether this error is retryable
    pub retryable: bool,
    /// Suggested retry delay in seconds (if retryable)
    pub retry_after_secs: Option<u64>,
    /// The URL/endpoint that was called.
    pub endpoint: Option<String>,
    /// HTTP method used.
    pub method: Option<String>,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.user_message)?;
        if let Some(ref action) = self.action {
            write!(f, " {}", action)?;
        }
        Ok(())
    }
}

impl std::error::Error for ApiError {}

/// API provider identification.
///
/// Holds the provider's display name (e.g., "fal.ai", "Replicate").
/// This is a simple string wrapper — no hardcoded provider variants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiProvider(pub String);

impl ApiProvider {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }
}

impl std::fmt::Display for ApiProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Types of API errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApiErrorKind {
    /// Authentication failed (401)
    Unauthorized,
    /// Payment required / out of credits (402)
    PaymentRequired,
    /// Access forbidden (403)
    Forbidden,
    /// Resource not found (404)
    NotFound,
    /// Invalid input / validation error (422)
    ValidationError,
    /// Rate limit exceeded (429)
    RateLimited,
    /// Server error (5xx)
    ServerError,
    /// Request timeout (504)
    Timeout,
    /// Model-specific error (OOM, etc.)
    ModelError,
    /// Network/connection error
    NetworkError,
    /// Unknown error type
    Unknown,
}

impl ApiError {
    /// Create an error from an HTTP status code and response body.
    pub fn from_response(
        provider: ApiProvider,
        status_code: u16,
        body: &str,
        endpoint: Option<&str>,
    ) -> Self {
        let (kind, user_message, action, retryable, retry_after) = match status_code {
            401 => (
                ApiErrorKind::Unauthorized,
                format!("{} API key is invalid or expired.", provider),
                Some(format!("Check your {} API key in Settings.", provider)),
                false,
                None,
            ),
            402 => (
                ApiErrorKind::PaymentRequired,
                format!("{} account is out of credits.", provider),
                Some(format!("Check your {} billing dashboard.", provider)),
                false,
                None,
            ),
            403 => (
                ApiErrorKind::Forbidden,
                format!("Access denied by {}.", provider),
                Some("Check your account permissions.".to_string()),
                false,
                None,
            ),
            404 => (
                ApiErrorKind::NotFound,
                format!("Resource not found on {}.", provider),
                endpoint.map(|e| format!("Endpoint {} may have changed.", e)),
                false,
                None,
            ),
            422 => {
                // Parse validation error from body if possible
                let detail = Self::extract_validation_detail(body);
                (
                    ApiErrorKind::ValidationError,
                    format!("Invalid request: {}", detail),
                    None,
                    false,
                    None,
                )
            }
            429 => (
                ApiErrorKind::RateLimited,
                format!("{} rate limit exceeded.", provider),
                Some("Request will be retried automatically.".to_string()),
                true,
                Some(60),
            ),
            500 => (
                ApiErrorKind::ServerError,
                format!("{} server error.", provider),
                Some("This is temporary. Request will be retried.".to_string()),
                true,
                Some(errors::SERVER_ERROR_RETRY_DELAY_SECS),
            ),
            502 | 503 => (
                ApiErrorKind::ServerError,
                format!("{} service temporarily unavailable.", provider),
                Some("Request will be retried automatically.".to_string()),
                true,
                Some(errors::BAD_GATEWAY_RETRY_DELAY_SECS),
            ),
            504 => (
                ApiErrorKind::Timeout,
                format!("{} request timed out.", provider),
                Some("The model may be under heavy load. Retrying...".to_string()),
                true,
                Some(errors::GATEWAY_TIMEOUT_RETRY_DELAY_SECS),
            ),
            _ if status_code >= 500 => (
                ApiErrorKind::ServerError,
                format!("{} server error (HTTP {}).", provider, status_code),
                Some("Request will be retried.".to_string()),
                true,
                Some(errors::SERVER_ERROR_RETRY_DELAY_SECS),
            ),
            _ => (
                ApiErrorKind::Unknown,
                format!("{} error (HTTP {}).", provider, status_code),
                None,
                false,
                None,
            ),
        };

        Self {
            provider,
            kind,
            status_code: Some(status_code),
            raw_message: body.to_string(),
            user_message,
            action,
            retryable,
            retry_after_secs: retry_after,
            endpoint: None,
            method: None,
        }
    }

    /// Create an error from a model/processing failure.
    pub fn from_model_error(provider: ApiProvider, error: &str) -> Self {
        // Detect specific error patterns
        let (kind, user_message, action, retryable) = if patterns::OOM_PATTERNS
            .iter()
            .any(|pattern| error.contains(pattern))
        {
            (
                ApiErrorKind::ModelError,
                "Model ran out of memory.".to_string(),
                Some("Try a simpler prompt or smaller image.".to_string()),
                false,
            )
        } else if patterns::TIMEOUT_PATTERNS
            .iter()
            .any(|pattern| error.contains(pattern))
        {
            (
                ApiErrorKind::Timeout,
                "Model timed out during processing.".to_string(),
                Some("Request will be retried.".to_string()),
                true,
            )
        } else if patterns::HEALTH_CHECK_PATTERNS
            .iter()
            .any(|pattern| error.contains(pattern))
        {
            (
                ApiErrorKind::ServerError,
                "Model failed to start.".to_string(),
                Some("This is temporary. Request will be retried.".to_string()),
                true,
            )
        } else if error.contains("canceled") || error.contains("cancelled") {
            (
                ApiErrorKind::Unknown,
                "Request was canceled.".to_string(),
                None,
                false,
            )
        } else {
            (
                ApiErrorKind::ModelError,
                format!("Model error: {}", Self::truncate_message(error, 100)),
                None,
                false,
            )
        };

        Self {
            provider,
            kind,
            status_code: None,
            raw_message: error.to_string(),
            user_message,
            action,
            retryable,
            retry_after_secs: if retryable { Some(5) } else { None },
            endpoint: None,
            method: None,
        }
    }

    /// Create an error from a network failure.
    pub fn from_network_error(provider: ApiProvider, error: &reqwest::Error) -> Self {
        let (user_message, retryable) = if error.is_timeout() {
            ("Connection timed out.".to_string(), true)
        } else if error.is_connect() {
            (format!("Could not connect to {}.", provider), true)
        } else {
            (format!("Network error: {}", error), true)
        };

        Self {
            provider,
            kind: ApiErrorKind::NetworkError,
            status_code: None,
            raw_message: error.to_string(),
            user_message,
            action: Some("Check your internet connection.".to_string()),
            retryable,
            retry_after_secs: Some(5),
            endpoint: None,
            method: None,
        }
    }

    /// Extract validation detail from JSON error body.
    fn extract_validation_detail(body: &str) -> String {
        // Try to parse fal.ai style error
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
            // fal.ai format: {"detail": [{"msg": "...", "loc": [...]}]}
            if let Some(detail) = json.get("detail") {
                if let Some(arr) = detail.as_array()
                    && let Some(first) = arr.first()
                    && let Some(msg) = first.get("msg").and_then(|m| m.as_str())
                {
                    return msg.to_string();
                }
                if let Some(s) = detail.as_str() {
                    return s.to_string();
                }
            }
            // Generic error format: {"error": "..."}
            if let Some(msg) = json.get("error").and_then(|e| e.as_str()) {
                return msg.to_string();
            }
        }
        Self::truncate_message(body, 100)
    }

    /// Truncate a message to a maximum length (in characters, not bytes).
    fn truncate_message(msg: &str, max_len: usize) -> String {
        if msg.chars().count() <= max_len {
            msg.to_string()
        } else {
            let truncated: String = msg.chars().take(max_len).collect();
            format!("{}...", truncated)
        }
    }

    /// Check if the error is due to insufficient credits/payment.
    pub fn is_payment_error(&self) -> bool {
        self.kind == ApiErrorKind::PaymentRequired
    }

    /// Check if the error is a rate limit.
    pub fn is_rate_limited(&self) -> bool {
        self.kind == ApiErrorKind::RateLimited
    }

    /// Check if the error is an authentication issue.
    pub fn is_auth_error(&self) -> bool {
        self.kind == ApiErrorKind::Unauthorized
    }
}

/// Progress updates emitted during pipeline execution.
///
/// These can be used to update a CLI progress display or GUI progress bar.
#[derive(Debug, Clone)]
pub enum Progress {
    /// A pipeline stage has started.
    Started { stage: Stage },

    /// Waiting in queue (for async API calls).
    Queued { stage: Stage, position: u32 },

    /// Currently processing.
    Processing {
        stage: Stage,
        message: Option<String>,
    },

    /// A stage completed successfully.
    Completed { stage: Stage },

    /// A stage failed.
    Failed { stage: Stage, error: String },

    /// Retrying after a transient error.
    Retrying {
        stage: Stage,
        attempt: u32,
        max_attempts: u32,
        delay_secs: u64,
        reason: String,
    },

    /// File download progress.
    Downloading {
        stage: Stage,
        bytes_downloaded: u64,
        total_bytes: Option<u64>,
    },

    /// Log message from the API.
    Log { stage: Stage, message: String },

    /// Awaiting user approval before continuing.
    AwaitingApproval {
        stage: Stage,
        approval_data: ApprovalData,
    },
}

/// Response from the user for an approval step.
#[derive(Debug, Clone)]
pub enum ApprovalResponse {
    /// User approved — continue the pipeline.
    Approve,
    /// User rejected — cancel the pipeline.
    Reject,
    /// User wants to regenerate with the same prompt.
    Regenerate,
}

/// Data for approval steps in the pipeline.
#[derive(Debug, Clone)]
pub struct ApprovalData {
    /// Path to the generated image to approve.
    pub image_path: PathBuf,

    /// URL of the generated image.
    pub image_url: String,

    /// The prompt used to generate this image.
    pub prompt: String,

    /// Model used for generation.
    pub model: String,
}

impl Progress {
    /// Create a Started progress event without details.
    pub fn started(stage: Stage) -> Self {
        Progress::Started { stage }
    }

    pub fn queued(stage: Stage, position: u32) -> Self {
        Progress::Queued { stage, position }
    }

    pub fn processing(stage: Stage, message: Option<String>) -> Self {
        Progress::Processing { stage, message }
    }

    pub fn completed(stage: Stage) -> Self {
        Progress::Completed { stage }
    }

    pub fn failed(stage: Stage, error: String) -> Self {
        Progress::Failed { stage, error }
    }

    pub fn retrying(
        stage: Stage,
        attempt: u32,
        max_attempts: u32,
        delay_secs: u64,
        reason: String,
    ) -> Self {
        Progress::Retrying {
            stage,
            attempt,
            max_attempts,
            delay_secs,
            reason,
        }
    }

    pub fn downloading(stage: Stage, bytes_downloaded: u64, total_bytes: Option<u64>) -> Self {
        Progress::Downloading {
            stage,
            bytes_downloaded,
            total_bytes,
        }
    }

    /// Create a Log progress event without details.
    pub fn log(stage: Stage, message: String) -> Self {
        Progress::Log { stage, message }
    }

    /// Create an AwaitingApproval progress event.
    pub fn awaiting_approval(stage: Stage, approval_data: ApprovalData) -> Self {
        Progress::AwaitingApproval {
            stage,
            approval_data,
        }
    }
}

/// Pipeline stages for progress tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// Generating an image from text prompt.
    ImageGeneration,

    /// Converting an image to a 3D model.
    Model3DGeneration,

    /// Converting GLB to FBX using Blender.
    FbxConversion,

    /// Downloading a file from a URL.
    Download,
}

impl std::fmt::Display for Stage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Stage::ImageGeneration => write!(f, "Image Generation"),
            Stage::Model3DGeneration => write!(f, "3D Model Generation"),
            Stage::FbxConversion => write!(f, "FBX Conversion"),
            Stage::Download => write!(f, "Download"),
        }
    }
}

/// Output artifacts from a pipeline run.
#[derive(Debug, Clone, Default)]
pub struct PipelineOutput {
    /// The text prompt used (if any).
    pub prompt: Option<String>,

    /// Path to the output directory for this generation.
    pub output_dir: Option<PathBuf>,

    /// Path to the generated/input image.
    pub image_path: Option<PathBuf>,

    /// URL of the image (from API).
    pub image_url: Option<String>,

    /// Path to the generated 3D model (GLB).
    pub model_path: Option<PathBuf>,

    /// URL of the 3D model (from API).
    pub model_url: Option<String>,

    /// Path to the FBX file (if converted).
    pub fbx_path: Option<PathBuf>,

    /// Path to the textures directory (if extracted).
    pub textures_dir: Option<PathBuf>,
}

impl PipelineOutput {
    /// Create a new empty pipeline output.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the final model path.
    pub fn final_model_path(&self) -> Option<&PathBuf> {
        self.model_path.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::files::bundle as bundle_files;

    #[test]
    fn test_stage_display() {
        assert_eq!(Stage::ImageGeneration.to_string(), "Image Generation");
        assert_eq!(Stage::Model3DGeneration.to_string(), "3D Model Generation");
        assert_eq!(Stage::FbxConversion.to_string(), "FBX Conversion");
        assert_eq!(Stage::Download.to_string(), "Download");
    }

    #[test]
    fn test_pipeline_output_final_model() {
        let mut output = PipelineOutput::new();
        assert!(output.final_model_path().is_none());

        output.model_path = Some(PathBuf::from(bundle_files::MODEL_GLB));
        assert_eq!(
            output.final_model_path(),
            Some(&PathBuf::from(bundle_files::MODEL_GLB))
        );
    }

    #[test]
    fn test_api_provider_display() {
        assert_eq!(ApiProvider::new("fal.ai").to_string(), "fal.ai");
    }

    #[test]
    fn test_api_error_from_response_401() {
        let err = ApiError::from_response(ApiProvider::new("fal.ai"), 401, "unauthorized", None);
        assert_eq!(err.kind, ApiErrorKind::Unauthorized);
        assert!(!err.retryable);
        assert!(err.user_message.contains("invalid or expired"));
    }

    #[test]
    fn test_api_error_from_response_402_fal() {
        let err =
            ApiError::from_response(ApiProvider::new("fal.ai"), 402, "payment required", None);
        assert_eq!(err.kind, ApiErrorKind::PaymentRequired);
        assert!(!err.retryable);
        assert!(err.user_message.contains("fal.ai"));
        assert!(err.is_payment_error());
    }

    #[test]
    fn test_api_error_from_response_402_generic() {
        let err =
            ApiError::from_response(ApiProvider::new("fal.ai"), 402, "payment required", None);
        assert_eq!(err.kind, ApiErrorKind::PaymentRequired);
        assert!(err.user_message.contains("fal.ai"));
        assert!(err.is_payment_error());
    }

    #[test]
    fn test_api_error_from_response_403() {
        let err = ApiError::from_response(ApiProvider::new("fal.ai"), 403, "forbidden", None);
        assert_eq!(err.kind, ApiErrorKind::Forbidden);
        assert!(!err.retryable);
    }

    #[test]
    fn test_api_error_from_response_404() {
        let err = ApiError::from_response(
            ApiProvider::new("fal.ai"),
            404,
            "not found",
            Some("/v1/models"),
        );
        assert_eq!(err.kind, ApiErrorKind::NotFound);
        assert!(err.action.as_ref().unwrap().contains("/v1/models"));
    }

    #[test]
    fn test_api_error_from_response_422() {
        // Test with fal.ai style JSON error
        let body = r#"{"detail": [{"msg": "Invalid image format", "loc": ["body", "image"]}]}"#;
        let err = ApiError::from_response(ApiProvider::new("fal.ai"), 422, body, None);
        assert_eq!(err.kind, ApiErrorKind::ValidationError);
        assert!(err.user_message.contains("Invalid image format"));
    }

    #[test]
    fn test_api_error_from_response_422_string_detail() {
        let body = r#"{"detail": "Image too large"}"#;
        let err = ApiError::from_response(ApiProvider::new("fal.ai"), 422, body, None);
        assert!(err.user_message.contains("Image too large"));
    }

    #[test]
    fn test_api_error_from_response_429_rate_limit() {
        let err = ApiError::from_response(ApiProvider::new("fal.ai"), 429, "rate limited", None);
        assert_eq!(err.kind, ApiErrorKind::RateLimited);
        assert!(err.retryable);
        assert!(err.is_rate_limited());
        assert_eq!(err.retry_after_secs, Some(60));
        assert!(err.user_message.contains("rate limit"));
    }

    #[test]
    fn test_api_error_from_response_500() {
        let err = ApiError::from_response(ApiProvider::new("fal.ai"), 500, "internal error", None);
        assert_eq!(err.kind, ApiErrorKind::ServerError);
        assert!(err.retryable);
        assert_eq!(err.retry_after_secs, Some(5));
    }

    #[test]
    fn test_api_error_from_response_502_503() {
        for status in [502, 503] {
            let err =
                ApiError::from_response(ApiProvider::new("fal.ai"), status, "unavailable", None);
            assert_eq!(err.kind, ApiErrorKind::ServerError);
            assert!(err.retryable);
            assert_eq!(err.retry_after_secs, Some(10));
        }
    }

    #[test]
    fn test_api_error_from_response_504() {
        let err = ApiError::from_response(ApiProvider::new("fal.ai"), 504, "timeout", None);
        assert_eq!(err.kind, ApiErrorKind::Timeout);
        assert!(err.retryable);
        assert_eq!(err.retry_after_secs, Some(15));
    }

    #[test]
    fn test_api_error_from_response_unknown_5xx() {
        let err = ApiError::from_response(ApiProvider::new("fal.ai"), 599, "weird error", None);
        assert_eq!(err.kind, ApiErrorKind::ServerError);
        assert!(err.retryable);
    }

    #[test]
    fn test_api_error_from_response_unknown() {
        let err = ApiError::from_response(ApiProvider::new("fal.ai"), 418, "i'm a teapot", None);
        assert_eq!(err.kind, ApiErrorKind::Unknown);
        assert!(!err.retryable);
    }

    #[test]
    fn test_api_error_from_model_error_oom() {
        let err = ApiError::from_model_error(ApiProvider::new("fal.ai"), "OOM: out of memory");
        assert_eq!(err.kind, ApiErrorKind::ModelError);
        assert!(!err.retryable);
        assert!(err.user_message.contains("out of memory"));
    }

    #[test]
    fn test_api_error_from_model_error_timeout() {
        let err = ApiError::from_model_error(ApiProvider::new("fal.ai"), "task timeout exceeded");
        assert_eq!(err.kind, ApiErrorKind::Timeout);
        assert!(err.retryable);
    }

    #[test]
    fn test_api_error_from_model_error_health_check() {
        let err =
            ApiError::from_model_error(ApiProvider::new("fal.ai"), "E8765: health check failed");
        assert_eq!(err.kind, ApiErrorKind::ServerError);
        assert!(err.retryable);
    }

    #[test]
    fn test_api_error_from_model_error_canceled() {
        let err = ApiError::from_model_error(ApiProvider::new("fal.ai"), "task was canceled");
        assert_eq!(err.kind, ApiErrorKind::Unknown);
        assert!(!err.retryable);
    }

    #[test]
    fn test_api_error_from_model_error_generic() {
        let err = ApiError::from_model_error(ApiProvider::new("fal.ai"), "something went wrong");
        assert_eq!(err.kind, ApiErrorKind::ModelError);
        assert!(!err.retryable);
    }

    #[test]
    fn test_api_error_display() {
        let err = ApiError::from_response(ApiProvider::new("fal.ai"), 401, "bad key", None);
        let display = format!("{}", err);
        assert!(display.contains("invalid or expired"));
        assert!(display.contains("Settings"));
    }

    #[test]
    fn test_api_error_is_auth_error() {
        let auth_err = ApiError::from_response(ApiProvider::new("fal.ai"), 401, "", None);
        assert!(auth_err.is_auth_error());

        let other_err = ApiError::from_response(ApiProvider::new("fal.ai"), 500, "", None);
        assert!(!other_err.is_auth_error());
    }

    #[test]
    fn test_truncate_message() {
        assert_eq!(ApiError::truncate_message("short", 100), "short");
        let long = "a".repeat(150);
        let truncated = ApiError::truncate_message(&long, 100);
        assert_eq!(truncated.len(), 103); // 100 + "..."
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn test_error_enum_display() {
        let err = Error::MissingApiKey("FAL_KEY".to_string());
        assert!(err.to_string().contains("FAL_KEY"));

        let err = Error::InvalidModel("unknown".to_string());
        assert!(err.to_string().contains("unknown"));

        let err = Error::Pipeline("stage failed".to_string());
        assert!(err.to_string().contains("stage failed"));

        let err = Error::Validation("bad input".to_string());
        assert!(err.to_string().contains("bad input"));

        let err = Error::FileNotFound(PathBuf::from("/missing/file.glb"));
        assert!(err.to_string().contains("missing/file.glb"));
    }
}
