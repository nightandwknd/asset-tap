//! Machine-readable CLI interface (`--json`) — wire format v1.
//!
//! Implements [docs/CLI_MACHINE_INTERFACE.md]. The event structs here are
//! deliberately decoupled from core's `Progress`/`Stage`/`ApiErrorKind` types:
//! every wire name is an explicit string literal so internal renames can't
//! silently change the format external consumers parse.
//!
//! [docs/CLI_MACHINE_INTERFACE.md]: ../../docs/CLI_MACHINE_INTERFACE.md

use asset_tap_core::providers::{
    ParameterDef, ParameterType, ParameterWidget, ProviderCapability, ProviderRegistry,
};
use asset_tap_core::types::{ApiErrorKind, Error as CoreError, Progress, Stage};
use serde::Serialize;
use std::io::Write;

/// Interface version declared in the `start` event and catalog documents, as
/// a `"MAJOR.MINOR"` string (Terraform `format_version`-style semantics):
///
/// - **MAJOR** bumps on breaking wire-format changes (a field is removed, a
///   field's type/meaning changes, an event's required shape changes).
///   Consumers must reject an unknown MAJOR rather than guess at the shape.
/// - **MINOR** bumps on additive, backward-compatible changes (a new event
///   variant, a new optional field). Consumers should ignore unknown fields
///   and tolerate a MINOR higher than the one they were built against.
pub const INTERFACE_VERSION: &str = "1.0";

/// Exit code for usage errors (matches clap's default).
pub const EXIT_USAGE: u8 = 2;
/// Exit code for auth/key failures (spec §2).
pub const EXIT_AUTH: u8 = 3;
/// Exit code for provider/API errors (spec §2).
pub const EXIT_PROVIDER: u8 = 4;
/// Exit code for a canceled run (spec §2; --json mode).
pub const EXIT_CANCELED: u8 = 5;
/// Exit code for network/timeout failures (spec §2).
pub const EXIT_NETWORK: u8 = 6;
/// Exit code for local-environment failures (spec §2).
pub const EXIT_LOCAL: u8 = 7;
/// Conventional exit for signal interruption in human mode (128 + SIGINT).
/// The spec's exit-code table governs --json mode only; interactive users keep
/// the shell convention so wrappers detecting 130 still work.
pub const EXIT_SIGINT_HUMAN: u8 = 130;

// Wire error kinds (`result.kind`). Consumers treat unrecognized kinds as
// `unknown`, so new kinds may be added without an interface bump.
pub const KIND_MISSING_API_KEY: &str = "missing_api_key";
pub const KIND_UNAUTHORIZED: &str = "unauthorized";
pub const KIND_PAYMENT_REQUIRED: &str = "payment_required";
pub const KIND_FORBIDDEN: &str = "forbidden";
pub const KIND_NOT_FOUND: &str = "not_found";
pub const KIND_VALIDATION_ERROR: &str = "validation_error";
pub const KIND_RATE_LIMITED: &str = "rate_limited";
pub const KIND_SERVER_ERROR: &str = "server_error";
pub const KIND_TIMEOUT: &str = "timeout";
pub const KIND_MODEL_ERROR: &str = "model_error";
pub const KIND_NETWORK_ERROR: &str = "network_error";
pub const KIND_BLENDER_NOT_FOUND: &str = "blender_not_found";
pub const KIND_IO_ERROR: &str = "io_error";
pub const KIND_UNKNOWN: &str = "unknown";

/// One NDJSON event. Serialized as `{"event":"<variant>",...}`.
#[derive(Debug, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Start {
        interface: &'static str,
        generator: String,
    },
    Progress {
        stage: &'static str,
        state: &'static str,
        #[serde(flatten)]
        body: ProgressBody,
    },
    Log {
        level: &'static str,
        message: String,
    },
    Result {
        #[serde(flatten)]
        outcome: ResultOutcome,
    },
}

/// Terminal outcome of a run, flattened into the `result` event so the wire
/// shape stays `{"event":"result","status":"…", …fields}`. An enum (rather
/// than one struct of ten Options) so each status can only carry its own
/// fields — the compiler enforces what each result shape contains.
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ResultOutcome {
    Success {
        bundle_dir: String,
        duration_ms: u64,
    },
    Error {
        kind: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        stage: Option<&'static str>,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        action: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        retryable: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        retry_after_secs: Option<u64>,
    },
    Canceled {
        #[serde(skip_serializing_if = "Option::is_none")]
        stage: Option<&'static str>,
    },
}

/// State-specific optional fields of a `progress` event.
#[derive(Debug, Default, Serialize)]
pub struct ProgressBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes_downloaded: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_attempts: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl Event {
    pub fn start() -> Self {
        Event::Start {
            interface: INTERFACE_VERSION,
            generator: asset_tap_core::bundle::generator_string().to_string(),
        }
    }

    fn progress(stage: &'static str, state: &'static str, body: ProgressBody) -> Self {
        Event::Progress { stage, state, body }
    }

    pub fn result_success(bundle_dir: String, duration_ms: u64) -> Self {
        Event::Result {
            outcome: ResultOutcome::Success {
                bundle_dir,
                duration_ms,
            },
        }
    }

    pub fn result_error(err: WireError, stage: Option<Stage>) -> Self {
        Event::Result {
            outcome: ResultOutcome::Error {
                kind: err.kind,
                provider: err.provider,
                stage: stage.map(wire_stage),
                message: err.message,
                action: err.action,
                retryable: err.retryable,
                retry_after_secs: err.retry_after_secs,
            },
        }
    }

    pub fn result_canceled(stage: Option<Stage>) -> Self {
        Event::Result {
            outcome: ResultOutcome::Canceled {
                stage: stage.map(wire_stage),
            },
        }
    }
}

/// Serialize one event as a single NDJSON line on stdout and flush.
///
/// Flushing per line matters: with stdout piped (the only way `--json` is
/// consumed), the default block buffering would batch events and starve the
/// consumer of progress.
pub fn emit(event: &Event) {
    let mut out = std::io::stdout().lock();
    if serde_json::to_writer(&mut out, event).is_ok() {
        let _ = out.write_all(b"\n");
        let _ = out.flush();
    }
}

/// Wire name for a pipeline stage — single-sourced from core
/// (`Stage::wire_name`), so a stage rename can't silently diverge between
/// emitters.
pub fn wire_stage(stage: Stage) -> &'static str {
    stage.wire_name()
}

/// Map a core progress update to a wire event.
///
/// Returns `None` for updates with no wire representation
/// (`AwaitingApproval` — approval is a usage error under `--json`).
pub fn progress_event(progress: &Progress) -> Option<Event> {
    Some(match progress {
        Progress::Started { stage } => {
            Event::progress(wire_stage(*stage), "started", ProgressBody::default())
        }
        Progress::Queued { stage, position } => Event::progress(
            wire_stage(*stage),
            "queued",
            ProgressBody {
                position: Some(*position),
                ..Default::default()
            },
        ),
        Progress::Processing { stage, message } => Event::progress(
            wire_stage(*stage),
            "processing",
            ProgressBody {
                message: message.clone(),
                ..Default::default()
            },
        ),
        Progress::Downloading {
            stage,
            bytes_downloaded,
            total_bytes,
        } => Event::progress(
            wire_stage(*stage),
            "downloading",
            ProgressBody {
                bytes_downloaded: Some(*bytes_downloaded),
                total_bytes: *total_bytes,
                ..Default::default()
            },
        ),
        Progress::Retrying {
            stage,
            attempt,
            max_attempts,
            delay_secs,
            reason,
        } => Event::progress(
            wire_stage(*stage),
            "retrying",
            ProgressBody {
                attempt: Some(*attempt),
                max_attempts: Some(*max_attempts),
                delay_secs: Some(*delay_secs),
                reason: Some(reason.clone()),
                ..Default::default()
            },
        ),
        Progress::Completed { stage } => {
            Event::progress(wire_stage(*stage), "completed", ProgressBody::default())
        }
        Progress::Failed { stage, error } => Event::progress(
            wire_stage(*stage),
            "failed",
            ProgressBody {
                message: Some(error.clone()),
                ..Default::default()
            },
        ),
        Progress::Log { message, .. } => Event::Log {
            level: "info",
            message: message.clone(),
        },
        Progress::AwaitingApproval { .. } => return None,
    })
}

/// Error details for a `result` error event, decoupled from any error type.
#[derive(Debug)]
pub struct WireError {
    pub kind: &'static str,
    pub message: String,
    pub provider: Option<String>,
    pub action: Option<String>,
    pub retryable: Option<bool>,
    pub retry_after_secs: Option<u64>,
}

impl WireError {
    pub fn bare(kind: &'static str, message: String) -> Self {
        WireError {
            kind,
            message,
            provider: None,
            action: None,
            retryable: None,
            retry_after_secs: None,
        }
    }
}

/// An error with a pre-assigned wire kind.
///
/// Used at CLI validation sites (missing API key, output dir problems) where
/// the error is built as a formatted message rather than a core error type.
/// `Display` is just the message, so human-mode output is unchanged.
#[derive(Debug)]
pub struct KindedError {
    pub kind: &'static str,
    pub message: String,
}

impl std::fmt::Display for KindedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for KindedError {}

/// A local usage error: bad flags or `--param` names/values, detected before
/// any pipeline work starts.
///
/// Spec §2 maps these to exit 2 and allows them to exit *before* the `start`
/// event, so they never produce a `result` — the same shape a clap usage error
/// already has. Kept distinct from [`KindedError`] because no wire `kind`
/// describes an invalid invocation: `unknown` exits 1, which reads as a
/// retryable internal failure.
#[derive(Debug)]
pub struct UsageError {
    pub message: String,
}

impl std::fmt::Display for UsageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for UsageError {}

/// Find a [`UsageError`] in the cause chain, if any.
pub fn find_usage_error(err: &anyhow::Error) -> Option<&UsageError> {
    err.chain()
        .find_map(|cause| cause.downcast_ref::<UsageError>())
}

/// True when the error represents a cancellation — user signal, image
/// rejection, or a provider-side cancel. Typed (downcast to core's
/// `Error::is_cancellation`) rather than matching message text, so a
/// core-side copyedit can't silently reclassify cancels as errors.
pub fn is_cancellation(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<CoreError>()
            .is_some_and(CoreError::is_cancellation)
    })
}

/// Classify any error into wire error details.
///
/// Walks the cause chain looking for a [`KindedError`] or a core error;
/// anything else is `unknown`.
pub fn classify_error(err: &anyhow::Error) -> WireError {
    for cause in err.chain() {
        if let Some(kinded) = cause.downcast_ref::<KindedError>() {
            return WireError::bare(kinded.kind, kinded.message.clone());
        }
        if let Some(core_err) = cause.downcast_ref::<CoreError>() {
            return classify_core_error(core_err);
        }
    }
    WireError::bare(KIND_UNKNOWN, format!("{err:#}"))
}

fn classify_core_error(err: &CoreError) -> WireError {
    match err {
        CoreError::ApiError(api) => WireError {
            kind: wire_api_error_kind(api.kind),
            message: api.user_message.clone(),
            provider: Some(api.provider.0.clone()),
            action: api.action.clone(),
            retryable: Some(api.retryable),
            retry_after_secs: api.retry_after_secs,
        },
        CoreError::MissingApiKey(_) => WireError::bare(KIND_MISSING_API_KEY, err.to_string()),
        CoreError::Http(e) => {
            let kind = if e.is_timeout() {
                KIND_TIMEOUT
            } else {
                KIND_NETWORK_ERROR
            };
            WireError::bare(kind, err.to_string())
        }
        CoreError::Io(_) | CoreError::FileNotFound(_) => {
            WireError::bare(KIND_IO_ERROR, err.to_string())
        }
        CoreError::InvalidModel(_) | CoreError::Validation(_) => {
            WireError::bare(KIND_VALIDATION_ERROR, err.to_string())
        }
        // Cancellations are intercepted by is_cancellation() before
        // classification; this arm exists only for match exhaustiveness.
        CoreError::Cancelled => WireError::bare(KIND_UNKNOWN, err.to_string()),
        CoreError::Api(_) | CoreError::Json(_) | CoreError::Pipeline(_) | CoreError::Config(_) => {
            WireError::bare(KIND_UNKNOWN, err.to_string())
        }
    }
}

fn wire_api_error_kind(kind: ApiErrorKind) -> &'static str {
    match kind {
        ApiErrorKind::Unauthorized => KIND_UNAUTHORIZED,
        ApiErrorKind::PaymentRequired => KIND_PAYMENT_REQUIRED,
        ApiErrorKind::Forbidden => KIND_FORBIDDEN,
        ApiErrorKind::NotFound => KIND_NOT_FOUND,
        ApiErrorKind::ValidationError => KIND_VALIDATION_ERROR,
        ApiErrorKind::RateLimited => KIND_RATE_LIMITED,
        ApiErrorKind::ServerError => KIND_SERVER_ERROR,
        ApiErrorKind::Timeout => KIND_TIMEOUT,
        ApiErrorKind::ModelError => KIND_MODEL_ERROR,
        ApiErrorKind::NetworkError => KIND_NETWORK_ERROR,
        // Intercepted by is_cancellation() before classification; exists for
        // match exhaustiveness only.
        ApiErrorKind::Cancelled => KIND_UNKNOWN,
        ApiErrorKind::Unknown => KIND_UNKNOWN,
    }
}

/// Process exit code for a wire error kind (spec §2).
pub fn exit_code_for_kind(kind: &str) -> u8 {
    match kind {
        KIND_MISSING_API_KEY | KIND_UNAUTHORIZED => EXIT_AUTH,
        KIND_PAYMENT_REQUIRED
        | KIND_FORBIDDEN
        | KIND_NOT_FOUND
        | KIND_VALIDATION_ERROR
        | KIND_RATE_LIMITED
        | KIND_SERVER_ERROR => EXIT_PROVIDER,
        KIND_NETWORK_ERROR | KIND_TIMEOUT => EXIT_NETWORK,
        KIND_BLENDER_NOT_FOUND | KIND_IO_ERROR => EXIT_LOCAL,
        _ => 1,
    }
}

/// Wire document for `--version --json`: `{"version":"<calver>","interface":"1.0"}`.
/// A typed struct (rather than `serde_json::json!`) so field order is
/// guaranteed — the workspace doesn't enable serde_json's `preserve_order`
/// feature, so a `Value`-based map would serialize keys alphabetically.
#[derive(Debug, Serialize)]
pub struct VersionDoc {
    pub version: &'static str,
    pub interface: &'static str,
}

// ---------------------------------------------------------------------------
// Catalog output (`--list-providers --json`, `--list --json`)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct Catalog {
    pub interface: &'static str,
    pub providers: Vec<CatalogProvider>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub templates: Option<Vec<CatalogTemplate>>,
}

#[derive(Debug, Serialize)]
pub struct CatalogProvider {
    pub id: String,
    pub name: String,
    pub description: String,
    pub configured: bool,
    pub required_env_vars: Vec<String>,
    pub models: Vec<CatalogModel>,
}

#[derive(Debug, Serialize)]
pub struct CatalogModel {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub modality: &'static str,
    pub is_default: bool,
    pub parameters: Vec<CatalogParameter>,
}

#[derive(Debug, Serialize)]
pub struct CatalogParameter {
    pub name: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "type")]
    pub param_type: &'static str,
    pub default: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub widget: Option<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct CatalogTemplate {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    pub variables: Vec<CatalogTemplateVariable>,
    pub examples: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct CatalogTemplateVariable {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub required: bool,
}

/// Build the catalog document from the live registry.
///
/// `include_templates` adds the `templates` array (`--list --json`).
pub fn build_catalog(registry: &ProviderRegistry, include_templates: bool) -> Catalog {
    let providers = registry
        .list_all()
        .iter()
        .map(|provider| {
            let meta = provider.metadata();
            let mut models = Vec::new();
            for (capability, modality) in [
                (ProviderCapability::TextToImage, "text_to_image"),
                (ProviderCapability::ImageTo3D, "image_to_3d"),
            ] {
                for model in provider.list_models(capability) {
                    models.push(CatalogModel {
                        id: model.id,
                        name: model.name,
                        description: model.description,
                        modality,
                        is_default: model.is_default,
                        parameters: model.parameters.iter().map(parameter_wire).collect(),
                    });
                }
            }
            CatalogProvider {
                id: meta.id.clone(),
                name: meta.name.clone(),
                description: meta.description.clone(),
                configured: provider.is_available(),
                required_env_vars: meta.required_env_vars.clone(),
                models,
            }
        })
        .collect();

    let templates = include_templates.then(|| {
        asset_tap_core::templates::list_templates()
            .iter()
            .filter_map(|id| asset_tap_core::templates::get_template_definition(id))
            .map(|t| CatalogTemplate {
                id: t.id,
                name: t.name,
                description: t.description,
                category: t.category,
                variables: t
                    .variables
                    .into_iter()
                    .map(|v| CatalogTemplateVariable {
                        name: v.name,
                        description: v.description,
                        required: v.required,
                    })
                    .collect(),
                examples: t.examples,
            })
            .collect()
    });

    Catalog {
        interface: INTERFACE_VERSION,
        providers,
        templates,
    }
}

/// Wire representation of a provider-YAML parameter definition.
pub fn parameter_wire(def: &ParameterDef) -> CatalogParameter {
    CatalogParameter {
        name: def.name.clone(),
        label: def.label.clone(),
        description: def.description.clone(),
        param_type: match def.param_type {
            ParameterType::Float => "float",
            ParameterType::Integer => "integer",
            ParameterType::Boolean => "boolean",
            ParameterType::String => "string",
            ParameterType::Select => "select",
        },
        default: def.default.clone(),
        min: def.min,
        max: def.max,
        step: def.step,
        options: def.options.clone(),
        widget: def.widget.map(|w| match w {
            ParameterWidget::Slider => "slider",
            ParameterWidget::Input => "input",
        }),
    }
}

/// Print a catalog as a single pretty-printed JSON document on stdout.
///
/// Writes via the raw handle and swallows write errors — `println!` would
/// PANIC on a broken pipe (e.g. `asset-tap --list --json | head -1`), which is
/// a routine way for tool/agent consumers to read a bounded amount.
pub fn print_catalog(catalog: &Catalog) {
    if let Ok(doc) = serde_json::to_string_pretty(catalog) {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(doc.as_bytes());
        let _ = out.write_all(b"\n");
        let _ = out.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use asset_tap_core::types::{ApiError, ApiProvider};

    #[test]
    fn typed_cancellation_is_detected() {
        let err: anyhow::Error = CoreError::Cancelled.into();
        assert!(is_cancellation(&err));
    }

    #[test]
    fn provider_side_cancel_is_detected() {
        // Regression: a fal job canceled server-side arrives as an ApiError
        // ("Request was canceled.", American spelling). It must classify as a
        // cancellation, not kind=unknown/exit 1.
        let api = ApiError::from_model_error(ApiProvider::new("fal.ai"), "task was canceled");
        let err: anyhow::Error = CoreError::from(api).into();
        assert!(is_cancellation(&err));
    }

    #[test]
    fn message_text_no_longer_drives_cancellation() {
        // Cancellation is typed; an ordinary error whose text merely contains
        // the old marker phrase must NOT be classified as a cancel.
        let err: anyhow::Error =
            CoreError::Pipeline("provider said: job cancelled by user upstream".into()).into();
        assert!(!is_cancellation(&err));
    }

    #[test]
    fn exit_codes_match_spec_table() {
        assert_eq!(exit_code_for_kind(KIND_MISSING_API_KEY), EXIT_AUTH);
        assert_eq!(exit_code_for_kind(KIND_RATE_LIMITED), EXIT_PROVIDER);
        assert_eq!(exit_code_for_kind(KIND_TIMEOUT), EXIT_NETWORK);
        assert_eq!(exit_code_for_kind(KIND_IO_ERROR), EXIT_LOCAL);
        assert_eq!(exit_code_for_kind("some_future_kind"), 1);
    }
}
