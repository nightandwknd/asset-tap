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
use indexmap::IndexMap;
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
///
/// History (the spec's Versioning section carries the full delta):
/// - `1.1`: bind `result` fields (`model`, `joints`, `vertices`, `clips`),
///   `bundle_dir` optional, `clips` in the catalog, the `clip list` /
///   `clip download` / `auth list` documents; the `fbx_conversion` stage and
///   `blender_not_found` kind are gone.
/// - `1.2`: catalog parameters carry `requires` / `conflicts_with`, the
///   conditions under which a parameter applies.
pub const INTERFACE_VERSION: &str = "1.2";

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
        /// Always present for `generate`. Absent for `bind`, which writes a
        /// model rather than producing a bundle.
        #[serde(skip_serializing_if = "Option::is_none")]
        bundle_dir: Option<String>,
        /// The written model. `bind` only.
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        /// Skin joint count. `bind` only.
        #[serde(skip_serializing_if = "Option::is_none")]
        joints: Option<usize>,
        /// Mesh vertex count. `bind` only.
        #[serde(skip_serializing_if = "Option::is_none")]
        vertices: Option<usize>,
        /// Animations the model now holds, in file order. `bind` only, and
        /// empty after `--fit-only`.
        #[serde(skip_serializing_if = "Option::is_none")]
        clips: Option<Vec<String>>,
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
                bundle_dir: Some(bundle_dir),
                model: None,
                joints: None,
                vertices: None,
                clips: None,
                duration_ms,
            },
        }
    }

    /// Terminal event for `bind --json`: a written model rather than a bundle.
    pub fn result_bind_success(
        model: String,
        joints: usize,
        vertices: usize,
        clips: Vec<String>,
        duration_ms: u64,
    ) -> Self {
        Event::Result {
            outcome: ResultOutcome::Success {
                bundle_dir: None,
                model: Some(model),
                joints: Some(joints),
                vertices: Some(vertices),
                clips: Some(clips),
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
/// Wire classification for a rig failure.
///
/// These are input problems, not transient ones: a clip that no installed pack
/// provides, a mesh that cannot be landmarked, a joint parked off the body.
/// Left to the generic path they became `unknown`, which exits 1 and reads as
/// a retryable internal failure — the opposite of the truth, and an agent
/// retrying a bind that can never succeed is the concrete cost.
pub fn classify_bind_error(err: &asset_tap_core::BindError) -> WireError {
    use asset_tap_core::BindError;
    // A pack problem is about the local machine, not the mesh, so it routes
    // through the same classifier `clip install` uses: an agent must not read
    // "no animation pack installed" as a provider error it can retry.
    if let BindError::Pack(pack) = err {
        return classify_pack_error(pack);
    }
    let kind = match err {
        BindError::Io { .. } => KIND_IO_ERROR,
        BindError::Gltf { .. } | BindError::OffMesh(_) | BindError::Failed(_) => {
            KIND_VALIDATION_ERROR
        }
        BindError::Pack(_) => unreachable!("handled above"),
    };
    // `to_string()` rather than anyhow's `{:#}`: `BindError`'s Display already
    // carries its cause, and the chained form repeated it verbatim.
    WireError::bare(kind, err.to_string())
}

/// Classify a clip-pack failure.
///
/// Every variant is about the local machine: a pack that is not installed
/// (`clip download` or `clip install`), or a file that is not an animation
/// library. None of them get better by retrying, which is what `unknown`
/// (exit 1) would imply.
pub fn classify_pack_error(err: &asset_tap_core::rig::ClipPackError) -> WireError {
    use asset_tap_core::rig::ClipPackError;
    let kind = match err {
        ClipPackError::Missing(_) | ClipPackError::NoLibrary(_) | ClipPackError::Install(_) => {
            KIND_IO_ERROR
        }
        // Both are fixed by installing the right pack, not by retrying and not
        // by editing the mesh, so they stay on the local-environment code.
        ClipPackError::NoAnimations(_) | ClipPackError::UnknownClip(_) => KIND_IO_ERROR,
    };
    WireError::bare(kind, err.to_string())
}

pub fn classify_error(err: &anyhow::Error) -> WireError {
    for cause in err.chain() {
        if let Some(kinded) = cause.downcast_ref::<KindedError>() {
            return WireError::bare(kinded.kind, kinded.message.clone());
        }
        if let Some(core_err) = cause.downcast_ref::<CoreError>() {
            return classify_core_error(core_err);
        }
        // Rig failures reach here whenever a human ran the command: the
        // `--json` paths classify at the call site, but human mode returns the
        // error to `main`. Both must land on the same exit code, or an agent
        // that shells out without `--json` sees exit 1 (retryable) for a mesh
        // that can never bind.
        if let Some(bind) = cause.downcast_ref::<asset_tap_core::BindError>() {
            return classify_bind_error(bind);
        }
        if let Some(pack) = cause.downcast_ref::<asset_tap_core::rig::ClipPackError>() {
            return classify_pack_error(pack);
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
        KIND_IO_ERROR => EXIT_LOCAL,
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

/// Single-document `--json clip download` success payload (not NDJSON).
#[derive(Debug, Serialize)]
pub struct ClipDownloadDocument {
    pub status: &'static str,
    pub installed: Vec<String>,
    pub already_exists: bool,
    pub packs_version: u32,
}

impl ClipDownloadDocument {
    pub fn success(installed: Vec<String>, already_exists: bool, packs_version: u32) -> Self {
        Self {
            status: "success",
            installed,
            already_exists,
            packs_version,
        }
    }
}

/// Single-document error payload (`{status: "error", kind, message}`) for
/// the `--json` subcommands that emit one JSON object rather than an NDJSON
/// stream: `clip download` and `clip list`. MCP `clip_download` returns the
/// same object as its tool error.
#[derive(Debug, Serialize)]
pub struct ErrorDocument {
    pub status: &'static str,
    pub kind: &'static str,
    pub message: String,
}

impl ErrorDocument {
    pub fn from_wire(wire: WireError) -> Self {
        Self {
            status: "error",
            kind: wire.kind,
            message: wire.message,
        }
    }

    /// `clip download` failure, classified by type ([`clip_download_error_kind`]).
    pub fn from_clip_download_error(err: &anyhow::Error) -> Self {
        Self {
            status: "error",
            kind: clip_download_error_kind(err),
            message: err.to_string(),
        }
    }
}

/// Classify a `clip download` failure onto the wire (`network_error` | `io_error`).
///
/// Typed, like [`classify_bind_error`]: the fetch layer reports a
/// [`ReleaseFetchError`](asset_tap_core::release_fetch::ReleaseFetchError)
/// whose variant says whether the network was the problem; a pack that
/// would not install is a [`ClipPackError`](asset_tap_core::rig::ClipPackError).
/// Anything else (a temp dir, a zip that will not open) is the local machine.
pub fn clip_download_error_kind(err: &anyhow::Error) -> &'static str {
    for cause in err.chain() {
        if let Some(fetch) =
            cause.downcast_ref::<asset_tap_core::release_fetch::ReleaseFetchError>()
        {
            return if fetch.is_network() {
                KIND_NETWORK_ERROR
            } else {
                KIND_IO_ERROR
            };
        }
        if let Some(pack) = cause.downcast_ref::<asset_tap_core::rig::ClipPackError>() {
            return classify_pack_error(pack).kind;
        }
    }
    KIND_IO_ERROR
}

// ---------------------------------------------------------------------------
// Catalog output (`--list-providers --json`, `--list --json`)
// ---------------------------------------------------------------------------

/// Where a provider's effective API key comes from. One resolution, used by
/// both the human `auth list` and `auth list --json` (spec §3) — the two
/// must never disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeySource {
    /// Stored in settings (`asset-tap auth set`). Wins over env.
    Stored,
    /// Present in the named environment variable.
    Env(String),
    Missing,
}

impl KeySource {
    pub const STORED: &'static str = "stored";
    pub const ENV: &'static str = "env";
    pub const MISSING: &'static str = "missing";

    /// Wire string for the `source` field.
    pub fn as_str(&self) -> &'static str {
        match self {
            KeySource::Stored => Self::STORED,
            KeySource::Env(_) => Self::ENV,
            KeySource::Missing => Self::MISSING,
        }
    }

    pub fn is_configured(&self) -> bool {
        !matches!(self, KeySource::Missing)
    }

    /// Resolve for one provider: stored key first, then the first non-empty
    /// required env var. Key *values* are never retained.
    pub fn resolve(
        provider_id: &str,
        required_env_vars: &[String],
        settings: &asset_tap_core::settings::Settings,
    ) -> Self {
        if settings
            .provider_api_keys
            .get(provider_id)
            .is_some_and(|k| !k.is_empty())
        {
            return KeySource::Stored;
        }
        required_env_vars
            .iter()
            .find(|var| std::env::var(var).is_ok_and(|v| !v.is_empty()))
            .map(|var| KeySource::Env(var.clone()))
            .unwrap_or(KeySource::Missing)
    }
}

/// `asset-tap auth list --json` (spec §3): which providers have an effective
/// API key and where it comes from. Never carries key material.
#[derive(Debug, Serialize)]
pub struct AuthCatalog {
    pub interface: &'static str,
    pub providers: Vec<AuthCatalogProvider>,
}

#[derive(Debug, Serialize)]
pub struct AuthCatalogProvider {
    pub id: String,
    pub name: String,
    pub configured: bool,
    /// `stored` | `env` (see `env_var`) | `missing` — `KeySource::as_str`.
    pub source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env_var: Option<String>,
    pub required_env_vars: Vec<String>,
}

impl AuthCatalogProvider {
    fn from_source(
        id: String,
        name: String,
        required_env_vars: Vec<String>,
        source: KeySource,
    ) -> Self {
        let env_var = match &source {
            KeySource::Env(var) => Some(var.clone()),
            _ => None,
        };
        AuthCatalogProvider {
            id,
            name,
            configured: source.is_configured(),
            source: source.as_str(),
            env_var,
            required_env_vars,
        }
    }
}

impl AuthCatalog {
    pub fn collect(
        registry: &ProviderRegistry,
        settings: &asset_tap_core::settings::Settings,
    ) -> Self {
        let providers = registry
            .list_all()
            .iter()
            .map(|p| {
                let meta = p.metadata();
                let source = KeySource::resolve(&meta.id, &meta.required_env_vars, settings);
                AuthCatalogProvider::from_source(
                    meta.id.clone(),
                    meta.name.clone(),
                    meta.required_env_vars.clone(),
                    source,
                )
            })
            .collect();
        AuthCatalog {
            interface: INTERFACE_VERSION,
            providers,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Catalog {
    pub interface: &'static str,
    pub providers: Vec<CatalogProvider>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub templates: Option<Vec<CatalogTemplate>>,
    /// Clip ids every installed pack provides (`--list --json` only) — the
    /// names `--clip` / MCP `clips[]` accept. Empty when no pack is installed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clips: Option<Vec<String>>,
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
    /// Sibling values this parameter needs in order to apply. Omitted when
    /// unconditional. A `null` value means "any value" — the sibling just has
    /// to be set. Sending the parameter when this isn't satisfied is a usage
    /// error (exit 2).
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub requires: IndexMap<String, serde_json::Value>,
    /// Sibling values that make this parameter invalid. Same `null` rule.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub conflicts_with: IndexMap<String, serde_json::Value>,
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
/// `include_templates` adds the `templates` and `clips` arrays (`--list
/// --json`): the full "what can I ask for" document, versus the
/// providers-only `--list-providers --json`.
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

    let clips = include_templates.then(|| {
        asset_tap_core::list_clips()
            .into_iter()
            .map(|c| c.id)
            .collect()
    });

    Catalog {
        interface: INTERFACE_VERSION,
        providers,
        templates,
        clips,
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
        requires: def.requires.clone(),
        conflicts_with: def.conflicts_with.clone(),
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

    #[test]
    fn clip_download_classifies_http_as_network_and_hash_as_io() {
        use asset_tap_core::release_fetch::ReleaseFetchError;
        let http: anyhow::Error = ReleaseFetchError::Status {
            what: "fetch release manifest",
            status: 404,
        }
        .into();
        assert_eq!(clip_download_error_kind(&http), KIND_NETWORK_ERROR);
        // Wrapped with context, as `download_clip_packs` callers may do.
        let wrapped = http.context("clip download failed");
        assert_eq!(clip_download_error_kind(&wrapped), KIND_NETWORK_ERROR);

        let hash: anyhow::Error = ReleaseFetchError::MissingHash.into();
        assert_eq!(clip_download_error_kind(&hash), KIND_IO_ERROR);
        assert_eq!(
            exit_code_for_kind(clip_download_error_kind(&hash)),
            EXIT_LOCAL
        );
        let integrity: anyhow::Error = ReleaseFetchError::Integrity("mismatch".into()).into();
        assert_eq!(clip_download_error_kind(&integrity), KIND_IO_ERROR);
    }

    /// Message text no longer drives classification: an untyped error whose
    /// text merely looks like a network failure stays local.
    #[test]
    fn clip_download_message_text_does_not_classify() {
        let fake = anyhow::anyhow!("HTTP 503: connection reset (from a zip entry name)");
        assert_eq!(clip_download_error_kind(&fake), KIND_IO_ERROR);
        let pack: anyhow::Error = asset_tap_core::rig::ClipPackError::Missing("ual1".into()).into();
        assert_eq!(clip_download_error_kind(&pack), KIND_IO_ERROR);
    }
}
