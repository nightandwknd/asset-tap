//! Model Context Protocol server: `asset-tap mcp` (stdio).
//!
//! A thin front door for MCP hosts (Claude Desktop, Cursor, IDE agents —
//! required where the host has no shell, useful alongside one where it
//! does). Every tool maps 1:1 onto something the CLI already
//! does and returns the same shapes the `--json` wire format uses
//! (`machine.rs`), so the two can't drift:
//!
//! | tool             | backed by                              |
//! |------------------|----------------------------------------|
//! | `list_catalog`   | `--list --json` (`machine::build_catalog`) |
//! | `auth_status`    | `auth list --json` (`machine::AuthCatalog`) |
//! | `inspect_bundle` | reads `bundle_dir/bundle.json`         |
//! | `clip_download`  | `--json clip download`                 |
//! | `generate`       | the generation run, exactly as `--json` |
//!
//! `generate` is deliberately implemented by building an **argv** from the
//! tool arguments and running it through the same clap `Cli` parser and the
//! same `run_generation` the binary uses. That means validation, model
//! resolution, `--param` routing, and error classification are literally the
//! CLI's; a usage error is word-for-word what the CLI would print. Progress
//! goes to MCP `notifications/progress` (when the host sends a progress
//! token) instead of stdout; cancellation comes from the request.
//!
//! stdout is the JSON-RPC transport: nothing in this process may print to it
//! while serving. Tracing already goes to stderr.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, Implementation, ProgressNotificationParam, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt, tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::machine;
use asset_tap_core::providers::ProviderRegistry;

pub const SERVER_NAME: &str = "asset-tap";
/// Tool-error kinds that exist only at this boundary (the wire's `result`
/// never describes an invalid invocation or a cancel as a *kind*; here a
/// tool error needs one so the model can branch on it).
pub const KIND_USAGE: &str = "usage";
pub const KIND_CANCELED: &str = "canceled";
/// Server instructions. The first sentence is [`APP_DESCRIPTION`] verbatim;
/// `cli/tests/positioning.rs` fails if the two drift apart.
///
/// [`APP_DESCRIPTION`]: asset_tap_core::constants::files::APP_DESCRIPTION
pub const INSTRUCTIONS: &str = "Asset Tap turns a prompt, an image, or a mesh into a game-ready \
asset: concept art, a textured GLB, humanoid rigging with animation clips, all in one bundle. \
Desktop app, CLI, and MCP server. Open source, with generation routed to your chosen provider. \
Typical flow: `auth_status` (do I have a key?) → \
    `list_catalog` (models, templates, parameters) → `generate` (returns the bundle directory) → \
    `inspect_bundle` (what's in it). A generation takes tens of seconds to minutes; progress is \
    reported via MCP progress notifications. Output is GLB. Pass `bind: true` to rig a humanoid; \
    the skeleton \
    is embedded, so rigging needs nothing installed. Add `clips: [...]` to bake animations in \
    (one model, N animations); a clip no installed pack provides is a local error. \
    Fetch the free Standard packs with `clip_download` (same document as \
    `--json clip download`); paid Source zips still use `clip install --from` on the CLI. \
    Errors carry `kind`, `retryable`, and an `action`. Retry only when retryable.";

/// The server is stateless; registry + settings are (re)loaded per call.
/// Provider keys are pushed into the process environment ONCE, at startup
/// (`serve_stdio`), exactly as the CLI and GUI do — env mutation isn't sound
/// once the runtime has other tasks live. Keys added with `auth set` while a
/// server is running are picked up on the host's next restart of it.
#[derive(Clone, Default)]
pub struct AssetTapServer;

/// Arguments for `generate`. Mirrors the CLI flags (see `asset-tap --help`);
/// each field becomes an argument to the same parser the CLI uses.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GenerateArgs {
    /// What to create. Required unless `image` is given.
    #[serde(default)]
    pub prompt: Option<String>,
    /// Skip image generation: use this local path or URL as the source image
    /// for the 3D stage (image-to-3D).
    #[serde(default)]
    pub image: Option<String>,
    /// Prompt template id (see `list_catalog` → templates); the prompt becomes
    /// the template's description.
    #[serde(default)]
    pub template: Option<String>,
    /// Provider id for both stages (e.g. `fal.ai`).
    #[serde(default)]
    pub provider: Option<String>,
    /// Text-to-image model id (see `list_catalog`).
    #[serde(default)]
    pub image_model: Option<String>,
    /// Image-to-3D model id (see `list_catalog`).
    #[serde(default)]
    pub model_3d: Option<String>,
    /// Model parameter overrides, e.g. {"guidance_scale": 7.0, "topology": "quad"}.
    /// Only parameters of the models that will actually run are accepted.
    #[serde(default)]
    pub params: Option<std::collections::BTreeMap<String, Value>>,
    /// Stop after image generation: an image-only bundle, no 3D model.
    #[serde(default)]
    pub image_only: bool,
    /// Output directory for the bundle (default: the configured output dir).
    #[serde(default)]
    pub output_dir: Option<String>,
    /// Bundle name to record in bundle.json (does not change the directory).
    #[serde(default)]
    pub name: Option<String>,
    /// Also copy the run's primary artifact (`model.glb`, or `image.png` under
    /// `image_only`) to this path. A path ending in that extension is the file
    /// verbatim; a directory receives `<name-or-bundle-dir>.<ext>`. Any other
    /// extension is a usage error. The bundle is written as usual either way.
    #[serde(default)]
    pub install: Option<String>,
    /// Rig the mesh to the embedded humanoid skeleton.
    #[serde(default)]
    pub bind: bool,
    /// Clips to bake in. The model gets one animation per clip, as Mixamo and
    /// Meshy do, rather than one export per clip. Ignored unless `bind`;
    /// defaults to `walk`. Names come from `list_catalog` → `clips`.
    #[serde(default)]
    pub clips: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ClipDownloadArgs {
    /// Replace Standard packs previously installed by `clip download`.
    /// Never overwrites a pack from `clip install` (Source or custom).
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct InspectBundleArgs {
    /// Absolute path to a bundle directory (the `bundle_dir` from `generate`).
    pub bundle_dir: String,
}

impl GenerateArgs {
    /// Translate to the CLI's argv. `--json` is always set: it makes the run
    /// fully non-interactive (implies `--yes`, rejects `--approve`) — the
    /// same contract an agent gets from the CLI.
    pub fn to_argv(&self) -> Vec<String> {
        let mut argv = vec!["asset-tap".to_string(), "--json".to_string()];
        if let Some(t) = &self.template {
            argv.push("--template".into());
            argv.push(t.clone());
        }
        if let Some(p) = &self.provider {
            argv.push("--provider".into());
            argv.push(p.clone());
        }
        if let Some(m) = &self.image_model {
            argv.push("--image-model".into());
            argv.push(m.clone());
        }
        if let Some(m) = &self.model_3d {
            argv.push("--3d-model".into());
            argv.push(m.clone());
        }
        if let Some(i) = &self.image {
            argv.push("--image".into());
            argv.push(i.clone());
        }
        if let Some(o) = &self.output_dir {
            argv.push("--output".into());
            argv.push(o.clone());
        }
        if let Some(n) = &self.name {
            argv.push("--name".into());
            argv.push(n.clone());
        }
        if let Some(i) = &self.install {
            argv.push("--install".into());
            argv.push(i.clone());
        }
        if self.image_only {
            argv.push("--image-only".into());
        }
        if self.bind {
            argv.push("--rig".into());
            // No clips named means no `--clip`: the default lives in
            // `PipelineConfig::bind_clips`, so the two non-interactive front
            // doors cannot drift onto different defaults. The GUI has no
            // generation-time rig option at all; rigging there is the Animate
            // panel, with the mesh on screen.
            for clip in self.clips.iter().flatten() {
                argv.push("--clip".into());
                argv.push(clip.clone());
            }
        }
        if let Some(params) = &self.params {
            for (k, v) in params {
                let s = match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                argv.push("--param".into());
                argv.push(format!("{k}={s}"));
            }
        }
        // Positional prompt last, after `--` so a prompt starting with '-' parses.
        if let Some(p) = &self.prompt {
            argv.push("--".into());
            argv.push(p.clone());
        }
        argv
    }
}

/// Registry + settings for one call. No env mutation here (see
/// [`AssetTapServer`]); `sync_env_once` did that before serving.
fn load_registry_and_settings() -> (ProviderRegistry, asset_tap_core::settings::Settings) {
    let registry = ProviderRegistry::new();
    let (settings, _status) = asset_tap_core::settings::Settings::load_with_status();
    (registry, settings)
}

/// Push saved provider keys into the environment the way `async_main` does,
/// so `DynamicProvider::is_configured()` (which reads env) sees keys saved via
/// `auth set` / the GUI. Called once, before the transport starts.
fn sync_env_once() {
    let registry = ProviderRegistry::new();
    let (mut settings, _status) = asset_tap_core::settings::Settings::load_with_status();
    if asset_tap_core::settings::is_dev_mode() {
        settings.sync_from_env(&registry);
    }
    settings.sync_to_env(&registry);
}

/// Success: structured content (for hosts that read it) + the same JSON as
/// text (for hosts that don't).
fn tool_json(value: Value) -> CallToolResult {
    CallToolResult::structured(value)
}

/// Tool-level error (`is_error: true`) with the wire-shaped payload — the
/// model can read `kind` / `retryable` / `action` and decide what to do.
fn tool_error(value: Value) -> CallToolResult {
    CallToolResult::structured_error(value)
}

#[tool_router]
impl AssetTapServer {
    pub fn new() -> Self {
        Self
    }

    #[tool(
        name = "list_catalog",
        description = "List available providers, models (with their parameter schemas), prompt templates, and the installed animation clip ids (`clips`, what `generate` accepts in `clips[]`). Same document as `asset-tap --list --json`.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = false)
    )]
    async fn list_catalog(&self) -> Result<CallToolResult, McpError> {
        let (registry, _settings) = load_registry_and_settings();
        let catalog = machine::build_catalog(&registry, true);
        Ok(tool_json(serde_json::to_value(catalog).map_err(|e| {
            McpError::internal_error(e.to_string(), None)
        })?))
    }

    #[tool(
        name = "auth_status",
        description = "Which providers have an effective API key and where it comes from (stored | env | missing). Never returns key material. Same document as `asset-tap auth list --json`.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = false)
    )]
    async fn auth_status(&self) -> Result<CallToolResult, McpError> {
        let (registry, settings) = load_registry_and_settings();
        let doc = machine::AuthCatalog::collect(&registry, &settings);
        Ok(tool_json(serde_json::to_value(doc).map_err(|e| {
            McpError::internal_error(e.to_string(), None)
        })?))
    }

    #[tool(
        name = "clip_download",
        description = "Fetch the free Standard animation packs from the latest GitHub Release (hash-verified). Same document as `asset-tap --json clip download`. Already-installed ids are left alone unless `force` is true, and force never replaces a pack installed via `clip install` (Source or custom).",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn clip_download(
        &self,
        Parameters(args): Parameters<ClipDownloadArgs>,
    ) -> Result<CallToolResult, McpError> {
        match asset_tap_core::download_clip_packs(args.force, |_| {}).await {
            Ok(asset_tap_core::ClipPacksDownloadResult::Downloaded { installed, version }) => {
                Ok(tool_json(
                    serde_json::to_value(machine::ClipDownloadDocument::success(
                        installed, false, version,
                    ))
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?,
                ))
            }
            Ok(asset_tap_core::ClipPacksDownloadResult::AlreadyExists { version }) => {
                Ok(tool_json(
                    serde_json::to_value(machine::ClipDownloadDocument::success(
                        Vec::new(),
                        true,
                        version,
                    ))
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?,
                ))
            }
            Err(err) => Ok(tool_error(
                serde_json::to_value(machine::ErrorDocument::from_clip_download_error(&err))
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?,
            )),
        }
    }

    #[tool(
        name = "inspect_bundle",
        description = "Read a bundle directory's bundle.json (v2 artifacts + pipeline, plus name/models/prompt/mesh stats) and list its files.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = false)
    )]
    async fn inspect_bundle(
        &self,
        Parameters(args): Parameters<InspectBundleArgs>,
    ) -> Result<CallToolResult, McpError> {
        let dir = std::path::PathBuf::from(&args.bundle_dir);
        let manifest_path = dir.join("bundle.json");
        let raw = match std::fs::read_to_string(&manifest_path) {
            Ok(s) => s,
            Err(e) => {
                return Ok(tool_error(json!({
                    "kind": machine::KIND_IO_ERROR,
                    "message": format!("cannot read {}: {e}", manifest_path.display()),
                    "action": "Pass the bundle_dir returned by generate.",
                    "retryable": false,
                })));
            }
        };
        let manifest: Value = serde_json::from_str(&raw)
            .map_err(|e| McpError::internal_error(format!("bundle.json parse: {e}"), None))?;
        let mut files: Vec<String> = Vec::new();
        collect_files(&dir, &dir, &mut files);
        files.sort();
        Ok(tool_json(json!({
            "bundle_dir": dir.to_string_lossy(),
            "files": files,
            "bundle": manifest,
        })))
    }

    #[tool(
        name = "generate",
        description = "Generate an asset from a text prompt (text → image → 3D) or from a reference image (image → 3D). Long-running (tens of seconds to minutes); progress is sent as MCP progress notifications. Returns the bundle directory; call inspect_bundle for its contents. Pass `install` to also drop the finished artifact at a path of your choosing. On failure returns kind/message/action/retryable exactly like the CLI's --json result.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    async fn generate(
        &self,
        Parameters(args): Parameters<GenerateArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        use clap::Parser;
        // Same parser as the binary: same flags, same validation, same
        // messages. Usage errors surface as tool errors (kind "usage").
        let cli = match crate::Cli::try_parse_from(args.to_argv()) {
            Ok(cli) => cli,
            Err(e) => {
                return Ok(tool_error(json!({
                    "kind": KIND_USAGE,
                    "message": e.to_string().trim(),
                    "action": "Fix the arguments (see list_catalog for models/templates/params).",
                    "retryable": false,
                })));
            }
        };
        // Same non-interactive input rule as `--json`, same message.
        if let Err(msg) = crate::non_interactive_input_check(&cli) {
            return Ok(tool_error(json!({
                "kind": KIND_USAGE,
                "message": msg,
                "action": "Pass `prompt` (text → image → 3D) or `image` (image → 3D).",
                "retryable": false,
            })));
        }
        let (registry, settings) = load_registry_and_settings();

        // Same pre-flight the CLI does before its `start` event.
        let params = match crate::resolve_param_overrides(&cli, &registry) {
            Ok(p) => p,
            Err(err) => return Ok(tool_error(usage_or_wire(&err))),
        };
        // `--install` shape is checked before the run for the same reason the
        // CLI checks it before `start`: a bad path must not cost a generation.
        if let Err(err) = crate::validate_install_path(&cli) {
            return Ok(tool_error(usage_or_wire(&err)));
        }

        // Progress → notifications/progress (only if the host asked for them).
        // One forwarder task drains an ordered channel, so notification N can
        // never overtake N-1 (a spawn-per-event would allow that).
        let progress_token = context.meta.get_progress_token();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let forwarder = progress_token.map(|token| {
            let peer = context.peer.clone();
            tokio::spawn(async move {
                let mut n: u64 = 0;
                while let Some(message) = rx.recv().await {
                    n += 1;
                    let mut param = ProgressNotificationParam::new(token.clone(), n as f64);
                    param.message = Some(message);
                    let _ = peer.notify_progress(param).await;
                }
            })
        });
        let mut on_progress = move |p: &asset_tap_core::types::Progress| {
            let _ = tx.send(asset_tap_core::progress_fmt::format_progress(p).message);
        };

        let mut last_stage = None;
        let started = std::time::Instant::now();
        let outcome = crate::run_generation(
            &cli,
            &settings,
            &registry,
            params,
            &mut last_stage,
            crate::RunSink::Embedded {
                on_progress: &mut on_progress,
                cancel: context.ct.clone(),
            },
        )
        .await;
        // Close the channel and let the forwarder flush, so every progress
        // notification is on the wire before the tool result.
        drop(on_progress);
        if let Some(f) = forwarder {
            let _ = f.await;
        }

        match outcome {
            Ok(output) => {
                let bundle_dir = output
                    .output_dir
                    .as_ref()
                    .map(|d| d.to_string_lossy().to_string());
                let bundle = bundle_dir
                    .as_ref()
                    .and_then(|d| {
                        std::fs::read_to_string(std::path::Path::new(d).join("bundle.json")).ok()
                    })
                    .and_then(|s| serde_json::from_str::<Value>(&s).ok());
                Ok(tool_json(json!({
                    "status": "success",
                    "bundle_dir": bundle_dir,
                    "duration_ms": started.elapsed().as_millis() as u64,
                    "bundle": bundle,
                })))
            }
            Err(err) if machine::is_cancellation(&err) => Ok(tool_error(json!({
                "status": "canceled",
                "kind": KIND_CANCELED,
                "stage": last_stage.map(machine::wire_stage),
                "message": "generation canceled",
                "retryable": true,
            }))),
            Err(err) => {
                let mut v = usage_or_wire(&err);
                if let Some(obj) = v.as_object_mut() {
                    obj.insert("status".into(), json!("error"));
                    if let Some(s) = last_stage {
                        obj.insert("stage".into(), json!(machine::wire_stage(s)));
                    }
                }
                Ok(tool_error(v))
            }
        }
    }
}

/// Usage errors keep their exact CLI text; everything else is classified onto
/// the wire error shape (`kind`, `provider`, `message`, `action`, `retryable`).
fn usage_or_wire(err: &anyhow::Error) -> Value {
    if let Some(usage) = machine::find_usage_error(err) {
        return json!({
            "kind": KIND_USAGE,
            "message": usage.to_string(),
            "action": "Fix the arguments (see list_catalog for models/templates/params).",
            "retryable": false,
        });
    }
    let w = machine::classify_error(err);
    let mut v = json!({ "kind": w.kind, "message": w.message });
    let obj = v.as_object_mut().expect("object");
    if let Some(p) = w.provider {
        obj.insert("provider".into(), json!(p));
    }
    if let Some(a) = w.action {
        obj.insert("action".into(), json!(a));
    }
    if let Some(r) = w.retryable {
        obj.insert("retryable".into(), json!(r));
    }
    if let Some(r) = w.retry_after_secs {
        obj.insert("retry_after_secs".into(), json!(r));
    }
    v
}

fn collect_files(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_files(root, &p, out);
        } else if let Ok(rel) = p.strip_prefix(root) {
            out.push(rel.to_string_lossy().to_string());
        }
    }
}

#[tool_handler]
impl ServerHandler for AssetTapServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(SERVER_NAME, env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }
}

/// `asset-tap mcp`: serve over stdio until the host disconnects.
pub async fn serve_stdio() -> anyhow::Result<()> {
    sync_env_once();
    let service = AssetTapServer::new()
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| anyhow::anyhow!("mcp: {e}"))?;
    service
        .waiting()
        .await
        .map_err(|e| anyhow::anyhow!("mcp: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn generate_args(clips: Option<Vec<&str>>) -> GenerateArgs {
        serde_json::from_value(serde_json::json!({
            "prompt": "a knight",
            "bind": true,
            "clips": clips,
        }))
        .unwrap()
    }

    /// `to_argv` builds a root invocation, so every option it emits has to be
    /// one the root parser accepts. `clips` was shipped as an array while the
    /// root `--clip` took a single value: two clips produced
    /// "cannot be used multiple times" (exit 2) instead of a two-animation
    /// model. Parsing the argv here is the only place that mismatch shows up
    /// without a live host.
    #[test]
    fn every_clip_survives_the_round_trip_to_the_cli_parser() {
        let argv = generate_args(Some(vec!["Walk_Loop", "Sword_Attack"])).to_argv();
        let cli = crate::Cli::try_parse_from(&argv).expect("argv must parse");
        assert_eq!(cli.clip, vec!["Walk_Loop", "Sword_Attack"]);
        assert!(cli.rig);
    }

    /// No `clips` passes no `--clip`: the argv asks for a rig and leaves the
    /// clip choice to the pipeline, which bakes `walk` by default (the same
    /// default `bind` without `--clip` applies). It is not a rig-only run;
    /// there is no `fit_only` in `GenerateArgs`.
    #[test]
    fn bind_without_clips_asks_for_a_rig_and_leaves_the_walk_default_to_the_pipeline() {
        let argv = generate_args(None).to_argv();
        let cli = crate::Cli::try_parse_from(&argv).expect("argv must parse");
        assert!(cli.rig);
        assert!(
            cli.clip.is_empty(),
            "no --clip: the pipeline default (walk) applies"
        );
        assert!(!argv.iter().any(|a| a == "--fit-only"));
    }

    /// `install` is a plain pass-through to `--install`; the root parser has
    /// to accept it, and it must not imply any of the flags `--install`
    /// conflicts with.
    #[test]
    fn install_reaches_the_cli_parser_as_a_path() {
        let args: GenerateArgs = serde_json::from_value(serde_json::json!({
            "prompt": "a knight",
            "install": "out/hero.glb",
        }))
        .unwrap();
        let argv = args.to_argv();
        let cli = crate::Cli::try_parse_from(&argv).expect("argv must parse");
        assert_eq!(
            cli.install.as_deref(),
            Some(std::path::Path::new("out/hero.glb"))
        );
    }
}
