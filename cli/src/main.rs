//! Asset Tap CLI
//!
//! Generate 3D models from text prompts.

#[cfg(feature = "mock")]
use asset_tap_core::constants::http::env;
use asset_tap_core::{
    config::{
        get_default_image_to_3d_model, get_default_text_to_image_model, list_image_to_3d_models,
        list_text_to_image_models,
    },
    convert::{convert_existing_models, convert_glb_to_fbx, is_blender_available},
    format_progress,
    pipeline::{PipelineConfig, run_pipeline},
    progress_fmt::stage_icon,
    providers::{ModelInfo, ParameterType, ProviderCapability, ProviderRegistry},
    settings::{get_output_dir, is_dev_mode},
    templates::{apply_template, list_templates},
    types::Progress,
};

use asset_tap::machine;
use clap::{Parser, Subcommand};
use std::collections::HashMap;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use walkdir::WalkDir;

#[cfg(feature = "mock")]
macro_rules! mock_example {
    () => {
        "  asset-tap \"test\" --mock --json               zero-cost pipeline test (no API calls)\n"
    };
}
#[cfg(not(feature = "mock"))]
macro_rules! mock_example {
    () => {
        ""
    };
}

/// Trailing help block (spec §7 Agent ergonomics). The `--mock` example is
/// only shown in builds that have the flag: an agent reading release `--help`
/// must never be pointed at an argument the binary rejects.
const AFTER_HELP: &str = concat!(
    "EXAMPLES:\n",
    "  asset-tap \"a stylized sci-fi crate\"          basic text-to-3D generation\n",
    "  asset-tap --image ref.png --no-fbx           image-to-3D, GLB only (no Blender)\n",
    "  asset-tap \"a crate\" --json --no-fbx -o ./out programmatic use: parse NDJSON events\n",
    "  asset-tap --list --json                      machine-readable model/template catalog\n",
    "  asset-tap auth list --json                   which providers have a key (preflight)\n",
    mock_example!(),
    "  echo $KEY | asset-tap auth set fal.ai        store a provider API key\n",
    "  asset-tap demo download                      fetch the showcase demo bundle\n",
    "\n",
    "AUTHENTICATION:\n",
    "  Provider keys resolve from stored settings first, then environment variables\n",
    "  (e.g. FAL_KEY). `asset-tap auth list` shows each provider's effective source.\n",
    "\n",
    "EXIT CODES:\n",
    "  0 ok · 1 other error · 2 usage · 3 auth/key · 4 provider · 5 canceled ·\n",
    "  6 network/timeout · 7 local environment (Blender, filesystem)\n",
    "\n",
    "For the full machine interface (NDJSON events, result contract, catalog schema),\n",
    "run: asset-tap --machine-help",
);

/// Asset Tap - Generate 3D models from text prompts
#[derive(Parser)]
#[command(name = "asset-tap")]
#[command(about = "Asset Tap - AI-powered text-to-3D generation")]
#[command(version)]
#[command(after_help = AFTER_HELP)]
struct Cli {
    /// Text prompt describing what to create (interactive if not provided)
    prompt: Option<String>,

    /// Auto-confirm the image approval step (skips the y/n/r prompt after image generation)
    #[arg(short = 'y', long)]
    yes: bool,

    /// Skip FBX conversion (GLB only)
    #[arg(long)]
    no_fbx: bool,

    /// Stop after image generation — produce an image-only bundle with no 3D model
    #[arg(long)]
    image_only: bool,

    /// Only convert existing GLB files to FBX (no API calls)
    #[arg(long)]
    convert_only: bool,

    /// Provider to use (e.g., fal.ai)
    #[arg(short = 'p', long, value_name = "PROVIDER")]
    provider: Option<String>,

    /// Image generation model
    #[arg(long, value_name = "MODEL")]
    image_model: Option<String>,

    /// 3D generation model
    #[arg(long = "3d-model", value_name = "MODEL")]
    model_3d: Option<String>,

    /// Skip image generation, use existing image (local path or URL)
    #[arg(long, value_name = "PATH")]
    image: Option<String>,

    /// Use a prompt template (prompt becomes the description)
    #[arg(short = 't', long, value_name = "NAME")]
    template: Option<String>,

    /// Output directory for generated assets (default: from settings, or ./output in dev mode)
    #[arg(short = 'o', long, value_name = "DIR")]
    output: Option<PathBuf>,

    /// List available models and templates
    #[arg(long)]
    list: bool,

    /// List available providers and their models
    #[arg(long)]
    list_providers: bool,

    /// Inspect a template's syntax and preview
    #[arg(long, value_name = "NAME")]
    inspect_template: Option<String>,

    /// Run in mock mode (simulated API responses, no costs)
    #[cfg(feature = "mock")]
    #[arg(long)]
    mock: bool,

    /// Add realistic delays in mock mode (simulates queue/processing time)
    #[cfg(feature = "mock")]
    #[arg(long, requires = "mock")]
    mock_delay: bool,

    /// Convert existing GLB files with WebP textures to use PNG textures
    #[arg(long)]
    convert_webp: bool,

    /// Require approval after image generation before proceeding to 3D (interactive mode only)
    #[arg(long)]
    approve: bool,

    /// Set a custom name for the generated bundle (or name an existing bundle with --export-bundle)
    #[arg(short = 'n', long, value_name = "NAME")]
    name: Option<String>,

    /// Export a bundle directory as a zip archive (requires --name if bundle is unnamed)
    #[arg(long, value_name = "BUNDLE_DIR")]
    export_bundle: Option<PathBuf>,

    /// Convert a specific GLB file or bundle directory to FBX (requires Blender)
    #[arg(long, value_name = "PATH")]
    convert_fbx: Option<PathBuf>,

    /// Set model parameter overrides (repeatable, e.g. --param guidance_scale=7.0 --param topology=quad)
    #[arg(long = "param", value_name = "KEY=VALUE")]
    params: Vec<String>,

    /// Emit machine-readable NDJSON events on stdout (implies --yes; run --machine-help for the full contract)
    ///
    /// Contract: stdout carries NDJSON only — one JSON object per line
    /// (`start`, `progress`, `log`, then exactly one `result`). All
    /// human-facing diagnostics go to stderr; never parse stderr. Implies
    /// --yes (fully non-interactive). Exit codes: 0 ok, 2 usage, 3 auth/key,
    /// 4 provider, 5 canceled, 6 network, 7 local environment, 1 other.
    /// Full spec (event fields, result shape, catalog schema): --machine-help
    #[arg(
        long,
        conflicts_with_all = [
            "approve",
            "convert_only",
            "convert_webp",
            "export_bundle",
            "convert_fbx",
            "inspect_template",
        ]
    )]
    json: bool,

    /// Print the machine-interface specification (NDJSON wire format, exit codes, catalog schema) and exit
    #[arg(long = "machine-help", alias = "describe", hide_short_help = true)]
    machine_help: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Manage stored provider API keys
    Auth {
        #[command(subcommand)]
        action: AuthAction,
    },
    /// Manage the showcase demo bundle
    Demo {
        #[command(subcommand)]
        action: DemoAction,
    },
}

#[derive(Subcommand)]
enum DemoAction {
    /// Download the demo bundle from the latest release.
    ///
    /// Fetches a small manifest first and skips the download when the current
    /// demo version already exists in the target directory. The archive's
    /// SHA-256 is verified against the manifest before extraction.
    Download {
        /// Target directory (defaults to the configured output directory)
        #[arg(short, long, value_name = "DIR")]
        output: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum AuthAction {
    /// Store an API key for a provider.
    ///
    /// If KEY is omitted, reads from stdin (pipe-friendly: `echo $K | asset-tap auth set fal.ai`)
    /// or prompts when stdin is a TTY.
    Set {
        /// Provider id (see `asset-tap auth list` or `asset-tap --list-providers`)
        provider: String,
        /// API key value. Omit to read from stdin.
        key: Option<String>,
    },
    /// Remove a stored API key for a provider.
    Remove {
        /// Provider id
        provider: String,
    },
    /// List providers and the source of their currently-effective API key.
    List {
        /// Emit a single JSON document instead of human text (see --machine-help §3).
        /// Key material is never included — only whether a key is present and where it comes from.
        #[arg(long)]
        json: bool,
    },
}

/// Print ASCII art banner
fn print_banner() {
    println!(concat!(
        "\n",
        "   ___               __    ______\n",
        "  / _ | ___ ___ ___ / /_  /_  __/__ ____\n",
        " / __ |(_-<(_-</ -_) __/   / / / _ `/ _ \\\n",
        "/_/ |_/___/___/\\__/\\__/   /_/  \\_,_/ .__/\n",
        "                                  /_/\n",
    ));
}

fn main() -> ExitCode {
    // Load .env file (before tokio runtime starts, so set_var is safe)
    dotenvy::dotenv().ok();

    // `--version --json`: emit a single JSON object instead of clap's built-in
    // human `--version` line. Must be checked on the raw args *before*
    // `Cli::parse()` — clap's derived `#[command(version)]` handler consumes
    // `--version` and exits the process before our code would otherwise see
    // it. Plain `--version` (no --json) is untouched: it falls through to
    // `Cli::parse()` below and keeps clap's stable single-line output.
    let raw_args: Vec<String> = std::env::args().collect();
    let has_version = raw_args.iter().any(|a| a == "--version" || a == "-V");
    let has_json = raw_args.iter().any(|a| a == "--json");
    if has_version && has_json {
        let doc = machine::VersionDoc {
            version: env!("CARGO_PKG_VERSION"),
            interface: machine::INTERFACE_VERSION,
        };
        if let Ok(line) = serde_json::to_string(&doc) {
            println!("{line}");
        }
        return ExitCode::SUCCESS;
    }

    let cli = Cli::parse();

    // Self-contained machine-interface documentation: agents/tools driving the
    // CLI have no repo checkout, so the spec ships inside the binary (spec §7).
    if cli.machine_help {
        print!("{}", include_str!("../../docs/CLI_MACHINE_INTERFACE.md"));
        return ExitCode::SUCCESS;
    }

    // Set mock env vars before tokio runtime starts (thread-safe)
    #[cfg(feature = "mock")]
    if cli.mock {
        // SAFETY: Called before tokio runtime starts — single-threaded, no concurrent env reads.
        unsafe {
            std::env::set_var(env::MOCK_API, "1");
            if cli.mock_delay {
                std::env::set_var(env::MOCK_DELAY, "1");
            }
        }
    }

    // Build and enter the tokio runtime
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("Error: failed to start async runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    let json_mode = cli.json;
    match rt.block_on(async_main(cli)) {
        Ok(code) => code,
        // The --json generation path emits its result event and returns an
        // exit code; an error escaping to here is pre-`start` or human mode.
        // Print the same "Error: ..." + cause chain anyhow's default handler
        // would, then map to the differentiated exit code (spec §2).
        Err(err) => {
            // Usage errors (bad flags, bad --param) print clap-style and exit
            // 2 without the anyhow cause chain: there is no internal failure to
            // report, only an invocation to correct.
            if let Some(usage) = machine::find_usage_error(&err) {
                eprintln!("error: {usage}");
                return ExitCode::from(machine::EXIT_USAGE);
            }
            eprintln!("Error: {:?}", err);
            let code = if machine::is_cancellation(&err) {
                // Spec §2 exit codes govern --json; interactive cancellation
                // keeps the shell convention (128 + SIGINT) so wrappers that
                // detect interruption via 130 keep working.
                if json_mode {
                    machine::EXIT_CANCELED
                } else {
                    machine::EXIT_SIGINT_HUMAN
                }
            } else {
                machine::exit_code_for_kind(machine::classify_error(&err).kind)
            };
            ExitCode::from(code)
        }
    }
}

async fn async_main(cli: Cli) -> anyhow::Result<ExitCode> {
    // Auth subcommands run an interactive prompt, so suppress INFO logs on
    // stderr — they'd drown out the "API key for ...:" prompt. File logging
    // still captures INFO for debugging.
    let quiet_console = matches!(
        cli.command,
        Some(Command::Auth { .. }) | Some(Command::Demo { .. })
    );
    let _guard = asset_tap_core::error_log::init_tracing(quiet_console);

    // Handle subcommands before any banner/pipeline setup. Auth commands
    // mutate settings.json directly and don't need the generation pipeline.
    if let Some(Command::Auth { action }) = cli.command {
        // clap can't express flag-vs-subcommand conflicts, so gate manually.
        if cli.json {
            eprintln!("error: '--json' cannot be used with the 'auth' subcommand");
            return Ok(ExitCode::from(machine::EXIT_USAGE));
        }
        return handle_auth(action).map(|_| ExitCode::SUCCESS);
    }

    // Demo subcommands fetch product artifacts from the latest release and
    // don't need the generation pipeline either.
    if let Some(Command::Demo { action }) = cli.command {
        if cli.json {
            eprintln!("error: '--json' cannot be used with the 'demo' subcommand");
            return Ok(ExitCode::from(machine::EXIT_USAGE));
        }
        return handle_demo(action).await;
    }

    // Show banner for main commands (not for --list, --inspect, or --json,
    // where stdout must stay machine-readable)
    if !cli.list
        && !cli.list_providers
        && cli.inspect_template.is_none()
        && !cli.convert_webp
        && !cli.json
    {
        print_banner();
    }

    // Handle --inspect-template flag (no registry needed)
    if let Some(template_name) = &cli.inspect_template {
        return handle_inspect_template(template_name).map(|_| ExitCode::SUCCESS);
    }

    // Handle --convert-webp flag (no registry needed)
    if cli.convert_webp {
        return handle_convert_webp(&cli.output).map(|_| ExitCode::SUCCESS);
    }

    // Handle --export-bundle flag (no registry needed)
    if let Some(ref bundle_dir) = cli.export_bundle {
        return handle_export_bundle(bundle_dir, &cli.output, cli.name.as_deref())
            .map(|_| ExitCode::SUCCESS);
    }

    // Handle --convert-fbx flag (no registry needed)
    if let Some(ref path) = cli.convert_fbx {
        return handle_convert_fbx(path).map(|_| ExitCode::SUCCESS);
    }

    // Handle mock mode
    #[cfg(feature = "mock")]
    if cli.mock {
        let msg = format!(
            "🎭 Running in mock mode{}",
            if cli.mock_delay { " (with delays)" } else { "" }
        );
        if cli.json {
            eprintln!("{msg}");
        } else {
            println!("{msg}");
        }
    }

    // Create provider registry once and reuse everywhere
    let registry = ProviderRegistry::new();

    // Load settings and sync GUI-saved API keys into the process environment so
    // DynamicProvider::is_configured() (which reads env vars) sees them. Without
    // this, the CLI only sees keys from .env / the shell — not ones saved via
    // the GUI settings UI — and every run in a release install would fail with
    // "No providers available" even though the GUI works fine.
    //
    // SAFETY: set_var is called here before any async task that reads these env
    // vars has been spawned; only this function holds the runtime at this point.
    use asset_tap_core::settings::Settings;
    let (mut settings, settings_status) = Settings::load_with_status();
    // Surface corruption to stderr so CLI users don't have to dig through
    // tracing logs to discover that their settings file just got moved aside.
    // The GUI shows the equivalent message (from the same shared method) as a
    // startup toast.
    if let Some(msg) = settings_status.user_message() {
        eprintln!("warning: {msg}");
    }
    if is_dev_mode() {
        settings.sync_from_env(&registry);
    }
    settings.sync_to_env(&registry);

    // Handle --list-providers flag
    if cli.list_providers {
        if cli.json {
            machine::print_catalog(&machine::build_catalog(&registry, false));
        } else {
            print_available_providers(&registry);
        }
        return Ok(ExitCode::SUCCESS);
    }

    // Handle --list flag
    if cli.list {
        if cli.json {
            machine::print_catalog(&machine::build_catalog(&registry, true));
        } else {
            print_available_options(&registry);
        }
        return Ok(ExitCode::SUCCESS);
    }

    // Show dev mode indicator
    if is_dev_mode() {
        if cli.json {
            eprintln!("🔧 Running in development mode (using ./output/)");
        } else {
            println!("🔧 Running in development mode (using ./output/)");
        }
    }

    // Handle --convert-only mode
    if cli.convert_only {
        return handle_convert_only(!cli.no_fbx).map(|_| ExitCode::SUCCESS);
    }

    // Surface a warning for any provider that's still unconfigured AFTER
    // sync_to_env has had a chance to populate env from settings. We do this
    // here (not during ProviderRegistry::new) so the check is accurate — at
    // registration time, settings hadn't been read yet and the result would
    // be a false alarm for users with GUI-saved keys.
    //
    // Skipped for `--list-providers` and `--list` because those commands
    // exit before reaching this point and already show per-provider state.
    registry.log_unconfigured_providers();

    // Validate `--param` before anything is emitted or generated: a bad
    // parameter name/value is a usage error (exit 2, no `start`/`result`), not
    // a failed run a consumer might retry.
    let params = resolve_param_overrides(&cli, &registry)?;

    if cli.json {
        // --json is non-interactive: a prompt (or --image) must come from the
        // args. This is a usage error, so it exits 2 before the start event.
        if cli.prompt.is_none() && cli.image.is_none() {
            eprintln!(
                "error: '--json' requires a prompt argument or '--image' \
                 (interactive prompting is disabled)"
            );
            return Ok(ExitCode::from(machine::EXIT_USAGE));
        }

        machine::emit(&machine::Event::start());
        let run_started = std::time::Instant::now();
        let mut last_stage = None;
        return Ok(
            match run_generation(&cli, &settings, &registry, params, &mut last_stage).await {
                Ok(output) => {
                    // bundle_dir is contractually absolute — refuse to emit a
                    // relative or missing path rather than silently violating
                    // the contract downstream consumers resolve against.
                    let resolved = output
                        .output_dir
                        .as_deref()
                        .ok_or_else(|| "pipeline reported no output directory".to_string())
                        .and_then(|dir| {
                            std::path::absolute(dir)
                                .map(|d| d.display().to_string())
                                .map_err(|e| format!("could not resolve bundle directory: {e}"))
                        });
                    match resolved {
                        Ok(bundle_dir) => {
                            machine::emit(&machine::Event::result_success(
                                bundle_dir,
                                run_started.elapsed().as_millis() as u64,
                            ));
                            ExitCode::SUCCESS
                        }
                        Err(message) => {
                            let wire = machine::WireError::bare(machine::KIND_IO_ERROR, message);
                            let code = machine::exit_code_for_kind(wire.kind);
                            machine::emit(&machine::Event::result_error(wire, last_stage));
                            ExitCode::from(code)
                        }
                    }
                }
                Err(err) if machine::is_cancellation(&err) => {
                    machine::emit(&machine::Event::result_canceled(last_stage));
                    ExitCode::from(machine::EXIT_CANCELED)
                }
                Err(err) => {
                    let wire = machine::classify_error(&err);
                    let code = machine::exit_code_for_kind(wire.kind);
                    machine::emit(&machine::Event::result_error(wire, last_stage));
                    ExitCode::from(code)
                }
            },
        );
    }

    run_generation(&cli, &settings, &registry, params, &mut None).await?;
    Ok(ExitCode::SUCCESS)
}

/// Run the full generation flow: validate keys and config, execute the
/// pipeline, relay progress (human print or NDJSON), and apply `--name`.
///
/// `last_stage` is updated as stages start so callers can attach stage
/// context to error/cancel results.
async fn run_generation(
    cli: &Cli,
    settings: &asset_tap_core::settings::Settings,
    registry: &ProviderRegistry,
    params: ParamOverrides,
    last_stage: &mut Option<asset_tap_core::types::Stage>,
) -> anyhow::Result<asset_tap_core::PipelineOutput> {
    // Validate API keys before prompting the user for input — otherwise the user
    // types a prompt only to hit a missing-key error with no actionable hint.
    validate_api_keys(settings, registry)?;

    // Build pipeline configuration
    let mut config = build_config(cli, settings)?;

    // Validate remaining requirements (output dir, etc.)
    validate_requirements(&config)?;

    // `--param` overrides were already validated and routed by the caller,
    // before any run started.
    if !params.image.is_empty() {
        config = config.with_image_model_params(params.image);
    }
    if !params.model_3d.is_empty() {
        config = config.with_3d_model_params(params.model_3d);
    }

    // Enable approval if: --approve flag OR settings require it (but not in
    // auto-confirm mode, not in image-only mode where there's no 3D stage to
    // approve continuing to, and never under --json which implies --yes).
    if (cli.approve || settings.require_image_approval) && !cli.yes && !cli.image_only && !cli.json
    {
        config = config.with_image_approval();
    }

    // Run the pipeline
    let (mut progress_rx, handle, approval_tx, cancel_tx) =
        run_pipeline(config.clone(), registry).await?;

    // Graceful cancellation (spec §4): the first SIGINT/SIGTERM asks the
    // pipeline to cancel; a second force-quits. Exit codes: 5 (spec §2) under
    // --json; conventional 130 (128+SIGINT) for interactive users so wrappers
    // detecting signal interruption keep working.
    let cancel_requested = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let force_exit_code = if cli.json {
        machine::EXIT_CANCELED
    } else {
        machine::EXIT_SIGINT_HUMAN
    };
    {
        let cancel_requested = cancel_requested.clone();
        tokio::spawn(async move {
            wait_for_shutdown_signal().await;
            cancel_requested.store(true, std::sync::atomic::Ordering::SeqCst);
            let _ = cancel_tx.send(());
            wait_for_shutdown_signal().await;
            // Force-quit skips destructors — flush stdout so already-emitted
            // lines (NDJSON events, human summaries) aren't lost.
            let _ = std::io::Write::flush(&mut std::io::stdout());
            std::process::exit(force_exit_code as i32);
        });
    }

    // Process progress updates
    while let Some(progress) = progress_rx.recv().await {
        // Track the stage currently in flight for error/cancel result context:
        // Started opens a stage, Completed closes it (an error between stages
        // must not blame the stage that already finished), Failed pins it.
        match &progress {
            asset_tap_core::types::Progress::Started { stage } => {
                *last_stage = Some(*stage);
            }
            asset_tap_core::types::Progress::Completed { .. } => {
                *last_stage = None;
            }
            asset_tap_core::types::Progress::Failed { stage, .. } => {
                *last_stage = Some(*stage);
            }
            _ => {}
        }
        if cli.json {
            if let Some(event) = machine::progress_event(&progress) {
                machine::emit(&event);
            }
        } else if let asset_tap_core::types::Progress::AwaitingApproval { approval_data, .. } =
            &progress
        {
            // Handle approval requests in CLI
            print_progress(&progress);
            let response = handle_cli_approval(approval_data)?;
            if let Some(tx) = &approval_tx {
                let _ = tx.send(response);
            }
        } else {
            print_progress(&progress);
        }
    }

    // Wait for pipeline to complete and get output
    let output = handle
        .await
        .map_err(|e| anyhow::anyhow!("Pipeline task failed: {}", e))??;

    // A cancel that lands after the pipeline's final cancel-flag check can
    // still complete the run — report it canceled, not success.
    if cancel_requested.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(asset_tap_core::types::Error::Cancelled.into());
    }

    // Apply --name to the generated bundle
    if let Some(ref name) = cli.name
        && let Some(ref dir) = output.output_dir
    {
        match asset_tap_core::bundle::load_bundle(dir) {
            Ok(mut bundle) => {
                if let Err(e) = bundle.rename(name.clone()) {
                    tracing::warn!("Failed to set bundle name: {}", e);
                }
            }
            Err(e) => tracing::warn!("Failed to load bundle for naming: {}", e),
        }
    }

    // Print summary (human mode only — --json reports via the result event)
    if !cli.json {
        print_summary(&output);
    }

    Ok(output)
}

/// Wait for a shutdown signal (SIGINT/ctrl-c, plus SIGTERM on unix).
async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        let mut sigterm =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(s) => s,
                Err(_) => {
                    let _ = tokio::signal::ctrl_c().await;
                    return;
                }
            };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = sigterm.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

/// Parse `KEY=VALUE` strings into a JSON value map.
///
/// Values are parsed as: booleans ("true"/"false"), integers, floats, or strings.
fn parse_param_values(raw: &[String]) -> anyhow::Result<HashMap<String, serde_json::Value>> {
    let mut map = HashMap::new();
    for entry in raw {
        let (key, val) = entry.split_once('=').ok_or_else(|| {
            anyhow::anyhow!("Invalid --param format: '{}' (expected KEY=VALUE)", entry)
        })?;
        let key = key.trim().to_string();
        let val = val.trim();
        if key.is_empty() {
            anyhow::bail!("Empty parameter name in --param '{}'", entry);
        }
        let json_val = match val {
            // Empty value means "unset" — e.g. `--param seed=` clears the
            // override and lets the provider apply its server-side default.
            "" => serde_json::Value::Null,
            "true" => serde_json::Value::Bool(true),
            "false" => serde_json::Value::Bool(false),
            _ => {
                if let Ok(i) = val.parse::<i64>() {
                    serde_json::json!(i)
                } else if let Ok(f) = val.parse::<f64>() {
                    if !f.is_finite() {
                        anyhow::bail!(
                            "Invalid parameter value for '{}': must be a finite number, got '{}'",
                            key,
                            val
                        );
                    }
                    serde_json::json!(f)
                } else {
                    serde_json::Value::String(val.to_string())
                }
            }
        };
        map.insert(key, json_val);
    }
    Ok(map)
}

/// Coerce a parsed JSON value to match the declared parameter type.
///
/// For example, `--param guidance_scale=7` parses as integer but the model
/// declares it as `float` — this converts `7` to `7.0` so the API gets the
/// expected type.
fn coerce_param_value(
    key: &str,
    value: &serde_json::Value,
    def: &asset_tap_core::providers::ParameterDef,
) -> anyhow::Result<serde_json::Value> {
    // A null value means "clear/unset" — pass through regardless of declared type.
    if value.is_null() {
        return Ok(serde_json::Value::Null);
    }
    let expected = &def.param_type;
    match expected {
        ParameterType::Float => match value {
            serde_json::Value::Number(n) => {
                let f = n.as_f64().ok_or_else(|| {
                    anyhow::anyhow!("Parameter '{}' expects a float, got '{}'", key, value)
                })?;
                Ok(serde_json::json!(f))
            }
            _ => anyhow::bail!("Parameter '{}' expects a float, got '{}'", key, value),
        },
        ParameterType::Integer => match value {
            serde_json::Value::Number(n) => {
                let i = n.as_i64().ok_or_else(|| {
                    anyhow::anyhow!("Parameter '{}' expects an integer, got '{}'", key, value)
                })?;
                Ok(serde_json::json!(i))
            }
            _ => anyhow::bail!("Parameter '{}' expects an integer, got '{}'", key, value),
        },
        ParameterType::Boolean => match value {
            serde_json::Value::Bool(_) => Ok(value.clone()),
            _ => anyhow::bail!("Parameter '{}' expects true/false, got '{}'", key, value),
        },
        ParameterType::String => match value {
            serde_json::Value::String(_) => Ok(value.clone()),
            _ => anyhow::bail!("Parameter '{}' expects a string, got '{}'", key, value),
        },
        // Select accepts any JSON scalar — options can be strings or numbers
        // (e.g. trellis-2's `resolution: [512, 1024, 1536]`). Validate against
        // the declared options list and auto-coerce string values to match the
        // option's type (so `--param resolution=512` works for numeric options).
        ParameterType::Select => {
            let Some(options) = def.options.as_ref() else {
                anyhow::bail!("Parameter '{}' is a select but has no options defined", key);
            };

            // Try direct match first (fast path for strings and exact types).
            if options.iter().any(|o| o == value) {
                return Ok(value.clone());
            }

            // Fall back to string-based comparison: `--param resolution=512`
            // parses as integer, but options are also numeric so normalize
            // both sides to strings and compare.
            let incoming = json_scalar_to_string(value);
            for opt in options {
                if json_scalar_to_string(opt) == incoming {
                    // Return in the option's native type (numeric options stay numeric).
                    return Ok(opt.clone());
                }
            }

            let opts_display: Vec<String> = options.iter().map(json_scalar_to_string).collect();
            anyhow::bail!(
                "Parameter '{}' value '{}' is not one of the allowed options: [{}]",
                key,
                incoming,
                opts_display.join(", ")
            );
        }
    }
}

fn json_scalar_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        _ => v.to_string(),
    }
}

/// Human-readable stage names used in `--param` diagnostics. Kept as constants
/// because the error text is asserted by tests and mirrored in two messages
/// each — the parameter list heading and the "doesn't apply" note.
const MODALITY_T2I: &str = "text-to-image";
const MODALITY_I23D: &str = "image-to-3D";

/// `--param` overrides after validation, split by the stage they belong to.
#[derive(Debug, Default)]
struct ParamOverrides {
    image: HashMap<String, serde_json::Value>,
    model_3d: HashMap<String, serde_json::Value>,
}

/// One pipeline stage's contribution to `--param` validation.
///
/// The two non-`Active` cases look identical to the validator — neither offers
/// parameters — but they mean opposite things to the user, so they stay
/// distinct: a skipped stage is expected, an unresolved one is a bad model id.
enum StageModel {
    /// The run will send requests to this model; its parameters are valid.
    Active(Box<ModelInfo>),
    /// Skipped by a flag: `--image` supplies the image, `--image-only` drops 3D.
    Skipped,
    /// No model resolved. Carries the id the user named, when they named one —
    /// otherwise no provider is configured at all. Either way the run fails
    /// later with a provider error; here it just contributes nothing.
    Unresolved(Option<String>),
}

impl StageModel {
    /// The model to validate against, if this stage has one.
    fn model(&self) -> Option<&ModelInfo> {
        match self {
            StageModel::Active(model) => Some(model),
            StageModel::Skipped | StageModel::Unresolved(_) => None,
        }
    }
}

/// The models a run will actually send requests to.
///
/// Only these contribute parameters. A stage the run skips has no say in what
/// `--param` accepts or in what an error lists.
struct ActiveModels {
    image: StageModel,
    model_3d: StageModel,
}

/// Resolve one stage's model the way the pipeline will.
///
/// Mirrors core's `resolve_provider` precedence — explicit `--image-model` /
/// `--3d-model` wins, otherwise the provider named by `-p` (falling back to the
/// registry default) picks its own default model for the capability. Resolving
/// via `get_default_*_model(registry)` instead would ignore `-p` entirely and
/// report another provider's parameters.
fn resolve_stage_model(
    registry: &ProviderRegistry,
    provider_id: Option<&str>,
    model_id: Option<&str>,
    capability: ProviderCapability,
) -> Option<ModelInfo> {
    if let Some(id) = model_id {
        return registry
            .find_provider_for_model(capability, id)
            // `find_provider_for_model` only searches *available* providers, so
            // fall back to the one named by `-p` when its key isn't configured.
            .or_else(|| provider_id.and_then(|p| registry.get(p)))
            // Match within the capability rather than via `get_model`, which
            // searches all of them — a 3D model id belongs to the 3D stage even
            // when it is passed to `--image-model`.
            .and_then(|p| p.list_models(capability).into_iter().find(|m| m.id == id));
    }

    let provider = match provider_id {
        Some(id) => registry.get(id)?,
        None => registry.get_default()?,
    };
    provider.get_default_model(capability).ok()
}

/// Resolve the models for the stages this invocation will actually run.
fn resolve_active_models(cli: &Cli, registry: &ProviderRegistry) -> ActiveModels {
    let provider = cli.provider.as_deref();

    let resolve = |skipped: bool, model_id: Option<&str>, capability| {
        if skipped {
            return StageModel::Skipped;
        }
        match resolve_stage_model(registry, provider, model_id, capability) {
            Some(model) => StageModel::Active(Box::new(model)),
            None => StageModel::Unresolved(model_id.map(str::to_string)),
        }
    };

    ActiveModels {
        image: resolve(
            cli.image.is_some(),
            cli.image_model.as_deref(),
            ProviderCapability::TextToImage,
        ),
        model_3d: resolve(
            cli.image_only,
            cli.model_3d.as_deref(),
            ProviderCapability::ImageTo3D,
        ),
    }
}

/// Parse, validate, coerce, and route `--param` overrides.
///
/// Runs before any pipeline work so a bad parameter is a usage error (exit 2,
/// no `start`/`result` events) rather than a failed run.
fn resolve_param_overrides(
    cli: &Cli,
    registry: &ProviderRegistry,
) -> anyhow::Result<ParamOverrides> {
    if cli.params.is_empty() {
        return Ok(ParamOverrides::default());
    }
    let active = resolve_active_models(cli, registry);

    // Nothing to validate against — no provider configured, or every named
    // model is unresolvable. Defer so the missing-key or invalid-model error
    // reports the actual problem instead of an unknown-parameter message.
    if active.image.model().is_none() && active.model_3d.model().is_none() {
        return Ok(ParamOverrides::default());
    }

    let resolved =
        parse_param_values(&cli.params).and_then(|parsed| route_params(&parsed, &active));

    // Every failure here is a mistyped invocation rather than a runtime fault,
    // so they all exit 2 — including the parse and coercion messages, which are
    // plain bails.
    resolved.map_err(|e| match machine::find_usage_error(&e) {
        Some(_) => e,
        None => usage_error(format!("{e:#}")),
    })
}

/// Validate, coerce, and route parsed parameters to the active models.
///
/// Each parameter must be declared by at least one model that this run will
/// actually use. Values are coerced to match the declared type (e.g., integer
/// → float).
fn route_params(
    params: &HashMap<String, serde_json::Value>,
    active: &ActiveModels,
) -> anyhow::Result<ParamOverrides> {
    if params.is_empty() {
        return Ok(ParamOverrides::default());
    }

    // Build name → ParameterDef lookup for each active model. A `fn` rather
    // than a closure so the borrow of `stage` outlives the call.
    fn param_defs(stage: &StageModel) -> HashMap<&str, &asset_tap_core::providers::ParameterDef> {
        stage
            .model()
            .map(|m| m.parameters.iter().map(|p| (p.name.as_str(), p)).collect())
            .unwrap_or_default()
    }
    let image_param_defs = param_defs(&active.image);
    let model_3d_param_defs = param_defs(&active.model_3d);

    let mut image_params = HashMap::new();
    let mut model_3d_params = HashMap::new();

    for (key, value) in params {
        let in_image = image_param_defs.get(key.as_str());
        let in_3d = model_3d_param_defs.get(key.as_str());

        match (in_image, in_3d) {
            (Some(def), None) => {
                let coerced = coerce_param_value(key, value, def)?;
                image_params.insert(key.clone(), coerced);
            }
            (None, Some(def)) => {
                let coerced = coerce_param_value(key, value, def)?;
                model_3d_params.insert(key.clone(), coerced);
            }
            (Some(image_def), Some(model_3d_def)) => {
                // Both models declare this param (e.g. 'resolution' exists on
                // nano-banana-2 as a string select AND on trellis-2 as a
                // numeric select). Coerce against each and route to whichever
                // the value actually fits. If both fit, warn and route to 3D.
                let image_fit = coerce_param_value(key, value, image_def);
                let model_3d_fit = coerce_param_value(key, value, model_3d_def);
                match (image_fit, model_3d_fit) {
                    (Ok(v), Err(_)) => {
                        image_params.insert(key.clone(), v);
                    }
                    (Err(_), Ok(v)) => {
                        model_3d_params.insert(key.clone(), v);
                    }
                    (Ok(_), Ok(v)) => {
                        eprintln!(
                            "  ⚠️  Parameter '{}' is valid for both image and 3D models; routing to 3D model",
                            key
                        );
                        model_3d_params.insert(key.clone(), v);
                    }
                    (Err(e_image), Err(e_3d)) => {
                        return Err(usage_error(format!(
                            "Parameter '{}' is declared by both image and 3D models but doesn't fit either:\n  image: {}\n  3D: {}",
                            key, e_image, e_3d
                        )));
                    }
                }
            }
            (None, None) => return Err(unknown_param_error(key, active)),
        }
    }

    Ok(ParamOverrides {
        image: image_params,
        model_3d: model_3d_params,
    })
}

/// Build a usage error (exit 2, no run events).
fn usage_error(message: String) -> anyhow::Error {
    anyhow::Error::new(machine::UsageError { message })
}

/// Report an unknown `--param` name against the models this run will actually
/// use, one section per active stage.
///
/// Skipped stages appear only in the closing note, never in the list of
/// parameters to try.
fn unknown_param_error(key: &str, active: &ActiveModels) -> anyhow::Error {
    fn section(stage: &StageModel, modality: &str) -> Option<String> {
        let model = stage.model()?;
        let mut names: Vec<&str> = model.parameters.iter().map(|p| p.name.as_str()).collect();
        names.sort_unstable();
        let body = if names.is_empty() {
            "  (this model declares no tunable parameters)".to_string()
        } else {
            names
                .iter()
                .map(|p| format!("  - {p}"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        Some(format!(
            "Valid parameters for {} ({}):\n{}",
            model.id, modality, body
        ))
    }

    /// Why a stage offers nothing — stated only when we actually know.
    fn note(stage: &StageModel, skipped: &str, modality: &str) -> Option<String> {
        match stage {
            StageModel::Active(_) => None,
            StageModel::Skipped => Some(skipped.to_string()),
            StageModel::Unresolved(Some(id)) => Some(format!(
                "No provider exposes the {modality} model '{id}', so its parameters can't be checked."
            )),
            StageModel::Unresolved(None) => None,
        }
    }

    let sections: Vec<String> = [
        section(&active.image, MODALITY_T2I),
        section(&active.model_3d, MODALITY_I23D),
    ]
    .into_iter()
    .flatten()
    .collect();

    let body = if sections.is_empty() {
        "No model is active for this run, so there are no parameters to set.".to_string()
    } else {
        sections.join("\n\n")
    };

    // Name the reason a stage offers nothing, so a parameter that belongs to a
    // skipped stage doesn't just look unsupported.
    let notes: Vec<String> = [
        note(
            &active.image,
            "This run uses a supplied image, so text-to-image parameters don't apply.",
            MODALITY_T2I,
        ),
        note(
            &active.model_3d,
            "This run is image-only, so image-to-3D parameters don't apply.",
            MODALITY_I23D,
        ),
    ]
    .into_iter()
    .flatten()
    .collect();

    let hint = if notes.is_empty() {
        String::new()
    } else {
        format!("\n\nNote: {}", notes.join(" "))
    };

    usage_error(format!("Unknown parameter '{key}'.\n\n{body}{hint}"))
}

fn build_config(
    cli: &Cli,
    settings: &asset_tap_core::settings::Settings,
) -> anyhow::Result<PipelineConfig> {
    // Get user input and expand template if specified.
    //
    // Prompt sources, in order:
    //   1. Prompt arg — always wins.
    //   2. --image — prompt isn't needed.
    //   3. Stdin, but only if it's a TTY. Piped/non-TTY stdin (CI, scripts,
    //      `asset-tap < /dev/null`) errors out instead of hanging or silently
    //      reading whatever happens to be on the pipe.
    let user_input = match (&cli.prompt, &cli.template) {
        (Some(p), _) => p.trim().to_string(),
        (None, _) if cli.image.is_some() => String::new(),
        (None, _) if !io::stdin().is_terminal() => {
            anyhow::bail!(
                "No prompt provided. Pass a prompt as an argument:\n    \
                 asset-tap \"a wooden treasure chest\""
            )
        }
        (None, _) => {
            print!("Describe what you want to create: ");
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            input.trim().to_string()
        }
    };

    let prompt = if let Some(ref t) = cli.template {
        apply_template(t, &user_input).ok_or_else(|| anyhow::anyhow!("Unknown template: {}", t))?
    } else {
        user_input.clone()
    };

    // Determine output directory: --output flag > settings/dev mode default
    let output_dir = cli.output.clone().unwrap_or_else(get_output_dir);

    // Build config
    let mut config = PipelineConfig::new().with_output_dir(output_dir);

    if let Some(ref image) = cli.image {
        // Validate local file paths before passing to pipeline
        if !image.starts_with("http://") && !image.starts_with("https://") {
            let path = std::path::Path::new(image);
            if !path.exists() {
                anyhow::bail!("Image file not found: {}", image);
            }
        }
        // Using a reference image — skip prompt/template since image generation is bypassed
        config = config.with_existing_image(image);
    } else {
        if !prompt.is_empty() {
            config = config.with_prompt(&prompt);
        }
        // Store original user input and template name when a template was used
        if let Some(ref t) = cli.template {
            if !user_input.is_empty() {
                config = config.with_user_prompt(&user_input);
            }
            config = config.with_template(t);
        }
        if let Some(ref model) = cli.image_model {
            config = config.with_image_model(model);
        }
    }

    if let Some(ref provider) = cli.provider {
        config = config
            .with_image_provider(provider)
            .with_3d_provider(provider);
    }

    if let Some(ref model) = cli.model_3d {
        config = config.with_3d_model(model);
    }

    if cli.no_fbx {
        config = config.without_fbx();
    }

    if cli.image_only {
        // `--image-only` skips the 3D stage. Combined with `--image` (which
        // already skips image *generation*), that would leave a pipeline with
        // nothing to do — reject the contradiction rather than silently
        // producing an empty run.
        if cli.image.is_some() {
            anyhow::bail!(
                "--image-only and --image can't be combined: --image already \
                 supplies the image and --image-only skips 3D generation, so \
                 there would be nothing to generate."
            );
        }
        config = config.with_skip_3d();
    }

    // Apply custom Blender path from settings
    if let Some(ref blender) = settings.blender_path
        && !blender.is_empty()
    {
        config = config.with_blender_path(blender);
    }

    Ok(config)
}

fn validate_requirements(config: &PipelineConfig) -> anyhow::Result<()> {
    // Validate output directory is set
    if config.output_dir.is_none() {
        return Err(anyhow::Error::new(machine::KindedError {
            kind: machine::KIND_IO_ERROR,
            message: "Output directory is required. Set it via:\n\
                1. --output flag: asset-tap --output /path/to/output \"prompt\"\n\
                2. Settings file (GUI): Configure in the application settings\n\
                3. Dev mode: Uses ./output/ by default"
                .to_string(),
        }));
    }

    // Validate output directory is not empty
    if let Some(ref dir) = config.output_dir
        && dir.as_os_str().is_empty()
    {
        return Err(anyhow::Error::new(machine::KindedError {
            kind: machine::KIND_IO_ERROR,
            message: "Output directory cannot be empty".to_string(),
        }));
    }

    Ok(())
}

fn validate_api_keys(
    settings: &asset_tap_core::settings::Settings,
    registry: &ProviderRegistry,
) -> anyhow::Result<()> {
    // Skip in mock mode
    #[cfg(feature = "mock")]
    if asset_tap_core::api::is_mock_mode() {
        return Ok(());
    }

    if !settings.has_required_api_keys(registry) {
        // Build dynamic error message from provider configs
        let mut env_vars: Vec<String> = Vec::new();
        let mut key_urls: Vec<String> = Vec::new();
        for provider in registry.list_all() {
            let meta = provider.metadata();
            for var in &meta.required_env_vars {
                if std::env::var(var).is_err() && !env_vars.contains(var) {
                    env_vars.push(var.clone());
                }
            }
            if let Some(url) = &meta.api_key_url
                && !key_urls.contains(url)
            {
                key_urls.push(url.clone());
            }
        }
        let env_list = env_vars.join(", ");
        let url_hint = if key_urls.is_empty() {
            String::new()
        } else {
            format!("\n\nGet API keys at: {}", key_urls.join(", "))
        };
        return Err(anyhow::Error::new(machine::KindedError {
            kind: machine::KIND_MISSING_API_KEY,
            message: format!(
                "API key(s) required: {env_list}\n\
                Set via:\n\
                1. Environment variable (e.g., {env_var}=your_key_here)\n\
                2. .env file\n\
                3. Settings file (GUI): Configure in the application settings{url_hint}",
                env_var = env_vars.first().unwrap_or(&"API_KEY".to_string()),
            ),
        }));
    }

    Ok(())
}

/// Handle `demo` subcommands.
///
/// Network failures map to the network exit code rather than the generic
/// error code, matching the exit-code table in the top-level help.
async fn handle_demo(action: DemoAction) -> anyhow::Result<ExitCode> {
    let DemoAction::Download { output } = action;
    let output_dir = output.unwrap_or_else(get_output_dir);
    fs::create_dir_all(&output_dir)?;

    println!("Checking demo bundle version...");
    match asset_tap_core::download_demo_bundle(output_dir.clone(), |_progress| {}).await {
        Ok(asset_tap_core::DemoDownloadResult::Downloaded(path)) => {
            println!("✅ Demo bundle downloaded to {}", path.display());
            Ok(ExitCode::SUCCESS)
        }
        Ok(asset_tap_core::DemoDownloadResult::AlreadyExists(version)) => {
            println!(
                "✅ Demo bundle v{} already present in {}",
                version,
                output_dir.display()
            );
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            eprintln!("error: demo download failed: {e:#}");
            Ok(ExitCode::from(machine::EXIT_NETWORK))
        }
    }
}

fn handle_auth(action: AuthAction) -> anyhow::Result<()> {
    use asset_tap_core::settings::Settings;

    let registry = ProviderRegistry::new();

    match action {
        AuthAction::Set { provider, key } => {
            let provider_id = validate_provider_id(&provider, &registry)?;
            let key = resolve_key_value(key, &provider_id)?;
            if key.is_empty() {
                anyhow::bail!("Refusing to store an empty key. Use `auth remove` to clear.");
            }

            let mut settings = Settings::load();
            settings.set_provider_api_key(&provider_id, key);
            settings
                .save()
                .map_err(|e| anyhow::anyhow!("Failed to save settings: {}", e))?;

            println!("✅ Stored API key for `{}`", provider_id);
            Ok(())
        }
        AuthAction::Remove { provider } => {
            let provider_id = validate_provider_id(&provider, &registry)?;
            let mut settings = Settings::load();
            let existed = settings.provider_api_keys.remove(&provider_id).is_some();
            if !existed {
                println!(
                    "ℹ️  No stored key for `{}` (nothing to remove)",
                    provider_id
                );
                return Ok(());
            }
            settings
                .save()
                .map_err(|e| anyhow::anyhow!("Failed to save settings: {}", e))?;
            println!("🗑️  Removed stored API key for `{}`", provider_id);
            Ok(())
        }
        AuthAction::List { json } => {
            let settings = Settings::load();
            // One resolution for both renderings (spec §3): the JSON document
            // and the human listing are views of the same collected catalog.
            let doc = machine::AuthCatalog::collect(&registry, &settings);
            if json {
                println!("{}", serde_json::to_string(&doc)?);
                return Ok(());
            }
            println!();
            println!("Provider API Keys");
            println!("{}", "=".repeat(60));
            if doc.providers.is_empty() {
                println!("No providers registered.");
                return Ok(());
            }
            for p in &doc.providers {
                let status = if p.configured {
                    "configured"
                } else {
                    "missing"
                };
                let source = match (p.source, &p.env_var) {
                    (machine::KeySource::ENV, Some(var)) => format!("env: {var}"),
                    (machine::KeySource::STORED, _) => "stored".to_string(),
                    _ => "—".to_string(),
                };
                println!("\n{} ({})", p.name, p.id);
                println!("  Status: {status}");
                println!("  Source: {source}");
                if !p.required_env_vars.is_empty() {
                    println!("  Env var(s): {}", p.required_env_vars.join(", "));
                }
            }
            println!();
            Ok(())
        }
    }
}

/// Confirm `provider` matches a registered provider id; otherwise list valid ones.
fn validate_provider_id(provider: &str, registry: &ProviderRegistry) -> anyhow::Result<String> {
    let valid: Vec<String> = registry
        .list_all()
        .iter()
        .map(|p| p.metadata().id.clone())
        .collect();
    if valid.iter().any(|id| id == provider) {
        Ok(provider.to_string())
    } else {
        anyhow::bail!(
            "Unknown provider `{}`. Valid ids: {}",
            provider,
            valid.join(", ")
        );
    }
}

/// Resolve a key value: inline arg wins; otherwise read stdin (piped) or prompt (TTY).
fn resolve_key_value(inline: Option<String>, provider_id: &str) -> anyhow::Result<String> {
    if let Some(k) = inline {
        return Ok(k.trim().to_string());
    }

    let stdin = io::stdin();
    if !stdin.is_terminal() {
        // Piped input: read entire stdin, strip trailing newline.
        let mut buf = String::new();
        stdin
            .lock()
            .read_to_string(&mut buf)
            .map_err(|e| anyhow::anyhow!("Failed to read stdin: {}", e))?;
        return Ok(buf.trim().to_string());
    }

    // Interactive: read with echo disabled so the key isn't visible on screen
    // or captured by terminal scrollback.
    let prompt = format!("API key for {}: ", provider_id);
    let key = rpassword::prompt_password(&prompt)
        .map_err(|e| anyhow::anyhow!("Failed to read input: {}", e))?;
    Ok(key.trim().to_string())
}

fn handle_convert_webp(output_override: &Option<PathBuf>) -> anyhow::Result<()> {
    let output_dir = output_override.clone().unwrap_or_else(get_output_dir);

    println!();
    println!("{}", "=".repeat(60));
    println!("  Convert GLB Files (WebP → PNG Textures)");
    println!("{}", "=".repeat(60));
    println!("\n  Scanning: {}", output_dir.display());
    println!();

    let report = batch_convert_output_dir(&output_dir)
        .map_err(|e| anyhow::anyhow!("Conversion failed: {}", e))?;

    report.print_summary();
    println!();

    Ok(())
}

fn handle_export_bundle(
    bundle_dir: &PathBuf,
    output_override: &Option<PathBuf>,
    name: Option<&str>,
) -> anyhow::Result<()> {
    use asset_tap_core::bundle::{export_bundle_zip, load_bundle};

    // Resolve bundle path (could be relative)
    let bundle_path = if bundle_dir.is_absolute() {
        bundle_dir.clone()
    } else {
        std::env::current_dir()?.join(bundle_dir)
    };

    if !bundle_path.is_dir() {
        anyhow::bail!("Bundle directory not found: {}", bundle_path.display());
    }

    // Load bundle and apply --name if provided
    let mut bundle = load_bundle(&bundle_path)?;
    if let Some(name) = name {
        bundle
            .rename(name.to_string())
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        println!("  Bundle named: {}", name);
    }

    // Require a name before export
    if bundle.metadata.name.is_none() {
        anyhow::bail!(
            "Bundle has no name. Use --name to set one:\n  \
             asset-tap --export-bundle {} --name \"My Asset\"",
            bundle_dir.display()
        );
    }
    let default_name = bundle.display_name().to_string();

    // Determine output path
    let dest = if let Some(out) = output_override {
        if out.extension().and_then(|e| e.to_str()) == Some("zip") {
            out.clone()
        } else {
            // Treat as directory, append filename
            out.join(format!("{}.zip", default_name))
        }
    } else {
        // Default: zip file next to the bundle directory
        bundle_path
            .parent()
            .unwrap_or(&bundle_path)
            .join(format!("{}.zip", default_name))
    };

    println!();
    println!("{}", "=".repeat(60));
    println!("  Export Bundle");
    println!("{}", "=".repeat(60));
    println!("\n  Source: {}", bundle_path.display());
    println!("  Dest:   {}", dest.display());
    println!();

    match export_bundle_zip(&bundle_path, &dest) {
        Ok(count) => {
            println!("  ✓ Exported {} files to {}", count, dest.display());
            println!();
        }
        Err(e) => {
            anyhow::bail!("Export failed: {}", e);
        }
    }

    Ok(())
}

fn handle_convert_fbx(path: &std::path::Path) -> anyhow::Result<()> {
    use asset_tap_core::constants::files::bundle as bundle_files;

    // Resolve path (could be relative)
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };

    // Determine the GLB file to convert
    let glb_path = if path.is_dir() {
        // Bundle directory — look for model.glb
        let glb = path.join(bundle_files::MODEL_GLB);
        if !glb.exists() {
            anyhow::bail!(
                "No {} found in bundle directory: {}",
                bundle_files::MODEL_GLB,
                path.display()
            );
        }
        glb
    } else if path.extension().and_then(|e| e.to_str()) == Some("glb") {
        if !path.exists() {
            anyhow::bail!("GLB file not found: {}", path.display());
        }
        path
    } else {
        anyhow::bail!(
            "Expected a .glb file or bundle directory, got: {}",
            path.display()
        );
    };

    // Check if FBX already exists
    let fbx_path = glb_path.with_extension("fbx");
    if fbx_path.exists() {
        println!("\n  ⚠️  FBX already exists: {}", fbx_path.display());
        println!("  Skipping conversion (delete the existing FBX to reconvert).");
        return Ok(());
    }

    // Load settings for custom Blender path
    let settings = asset_tap_core::settings::Settings::load();
    let custom_blender = settings.blender_path.as_deref();
    let has_custom_blender = custom_blender.is_some_and(|p| !p.is_empty());

    // Check Blender availability (auto-detected or custom path)
    if !is_blender_available() && !has_custom_blender {
        anyhow::bail!(
            "Blender is required for FBX conversion but was not found.\n\
            Install Blender from https://www.blender.org/download/ and ensure it's on your PATH."
        );
    }

    println!();
    println!("{}", "=".repeat(60));
    println!("  Convert GLB to FBX");
    println!("{}", "=".repeat(60));
    println!("\n  Source: {}", glb_path.display());

    match convert_glb_to_fbx(&glb_path, custom_blender)? {
        Some((fbx, textures_dir)) => {
            println!("  ✓ FBX:      {}", fbx.display());
            if let Some(ref tex) = textures_dir {
                println!("  ✓ Textures: {}", tex.display());
            }
            println!();
        }
        None => {
            anyhow::bail!("Blender is required for FBX conversion but was not found.");
        }
    }

    Ok(())
}

/// Scan output directory and convert all GLB files with WebP textures.
fn batch_convert_output_dir(output_dir: &std::path::Path) -> Result<BatchConvertReport, String> {
    let mut report = BatchConvertReport::default();

    // Find all GLB files in output directory
    let glb_files: Vec<PathBuf> = WalkDir::new(output_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("glb"))
        .map(|e| e.path().to_path_buf())
        .collect();

    report.total_files = glb_files.len();

    for glb_path in glb_files {
        println!("Checking: {}", glb_path.display());

        match asset_tap_core::glb_webp::convert_webp_to_png(&glb_path) {
            Ok(converted_data) => {
                // Check if file was actually modified (has WebP)
                let original_data = fs::read(&glb_path)
                    .map_err(|e| format!("Failed to read original file: {}", e))?;

                if converted_data.len() != original_data.len() || converted_data != original_data {
                    // File was converted, save it
                    fs::write(&glb_path, &converted_data)
                        .map_err(|e| format!("Failed to write converted file: {}", e))?;

                    println!("  ✓ Converted (WebP → PNG)");
                    report.converted_files.push(glb_path);
                } else {
                    println!("  • Skipped (no WebP textures)");
                    report.skipped_files += 1;
                }
            }
            Err(e) => {
                eprintln!("  ✗ Error: {}", e);
                report.failed_files.push((glb_path, e));
            }
        }
    }

    Ok(report)
}

/// Report of batch conversion results.
#[derive(Default)]
struct BatchConvertReport {
    total_files: usize,
    converted_files: Vec<PathBuf>,
    skipped_files: usize,
    failed_files: Vec<(PathBuf, String)>,
}

impl BatchConvertReport {
    fn print_summary(&self) {
        println!("\n=== Conversion Summary ===");
        println!("Total GLB files found: {}", self.total_files);
        println!("Converted: {}", self.converted_files.len());
        println!("Skipped (no WebP): {}", self.skipped_files);
        println!("Failed: {}", self.failed_files.len());

        if !self.failed_files.is_empty() {
            println!("\nFailed files:");
            for (path, error) in &self.failed_files {
                println!("  - {}: {}", path.display(), error);
            }
        }

        if !self.converted_files.is_empty() {
            println!("\nConverted files:");
            for path in &self.converted_files {
                println!("  - {}", path.display());
            }
        }
    }
}

fn handle_convert_only(export_fbx: bool) -> anyhow::Result<()> {
    println!();
    println!("{}", "=".repeat(60));
    println!("  Convert Existing Models");
    println!("{}", "=".repeat(60));

    if !export_fbx {
        println!("\n⚠️  FBX export disabled. Nothing to convert.");
        return Ok(());
    }

    let output_dir = get_output_dir();
    let (converted, skipped, failed) = convert_existing_models(&output_dir)?;

    println!();
    println!("{}", "-".repeat(40));
    println!("  Converted: {}", converted);
    println!("  Skipped:   {}", skipped);
    println!("  Failed:    {}", failed);
    println!();

    Ok(())
}

fn print_available_providers(registry: &ProviderRegistry) {
    // Single registry traversal shared with the --json catalog
    // (machine::build_catalog) so the human list and the machine catalog can't
    // drift — same providers, same models, same `is_default`/`configured`.
    let catalog = machine::build_catalog(registry, false);

    println!();
    println!("Available Providers");
    println!("{}", "=".repeat(60));

    let available: Vec<_> = catalog.providers.iter().filter(|p| p.configured).collect();
    if available.is_empty() {
        println!("\n⚠️  No providers available");
        println!("   Configure API key(s) in environment variables.");
        // List all providers and their required env vars
        for provider in &catalog.providers {
            if !provider.required_env_vars.is_empty() {
                println!(
                    "   - {} for {}",
                    provider.required_env_vars.join(", "),
                    provider.name
                );
            }
        }
        println!();
        return;
    }

    for provider in available {
        println!("\n{} - {}", provider.name, provider.description);
        println!("  ID: {} (-p {})", provider.id, provider.id);

        if !provider.required_env_vars.is_empty() {
            println!("  Env: {}", provider.required_env_vars.join(", "));
        }

        print_catalog_models(
            provider,
            "text_to_image",
            "Text-to-Image Models (--image-model)",
        );
        print_catalog_models(provider, "image_to_3d", "Image-to-3D Models (--3d-model)");
    }

    println!();
}

fn print_catalog_models(provider: &machine::CatalogProvider, modality: &str, heading: &str) {
    let models: Vec<_> = provider
        .models
        .iter()
        .filter(|m| m.modality == modality)
        .collect();
    if models.is_empty() {
        return;
    }
    println!("\n  {}:", heading);
    for model in models {
        let default_marker = if model.is_default { " (default)" } else { "" };
        let desc = model.description.as_deref().unwrap_or("");
        println!("    • {} - {}{}", model.id, desc, default_marker);
    }
}

/// Note: the "(default)" marker here is the EFFECTIVE default — what a run
/// uses when no model flag is given (first available provider's default) —
/// which is intentionally different from the catalog's per-provider
/// `is_default` (what a consumer preselects after choosing a provider).
fn print_available_options(registry: &ProviderRegistry) {
    println!();
    println!("Available Models and Templates");
    println!("{}", "=".repeat(40));

    println!("\nImage Models (--image-model):");
    let default_image = get_default_text_to_image_model(registry);
    for model in list_text_to_image_models(registry) {
        let marker = if Some(model.clone()) == default_image {
            " (default)"
        } else {
            ""
        };
        println!("  - {}{}", model, marker);
    }

    println!("\n3D Models (--3d-model):");
    let default_3d = get_default_image_to_3d_model(registry);
    for model in list_image_to_3d_models(registry) {
        let marker = if Some(model.clone()) == default_3d {
            " (default)"
        } else {
            ""
        };
        println!("  - {}{}", model, marker);
    }

    println!("\nPrompt Templates (-t, --template):");
    for template_name in list_templates() {
        use asset_tap_core::templates::get_template_definition;
        if let Some(template) = get_template_definition(&template_name) {
            let tag = if template.is_builtin {
                "[builtin]"
            } else {
                "[custom]"
            };
            println!("  - {} {} - {}", template_name, tag, template.description);
        } else {
            println!("  - {}", template_name);
        }
    }
    println!("  (Use --inspect-template <name> to view template syntax)");

    println!();
}

fn handle_inspect_template(name: &str) -> anyhow::Result<()> {
    use asset_tap_core::templates::get_template_definition;

    if let Some(template) = get_template_definition(name) {
        println!();
        println!("Template: {}", template.name);
        println!("{}", "=".repeat(60));
        println!();
        println!(
            "Type: {}",
            if template.is_builtin {
                "builtin"
            } else {
                "custom"
            }
        );
        println!("Description: {}", template.description);
        if let Some(source) = &template.source_path {
            println!("Source: {:?}", source);
        }
        println!();
        println!("Template Syntax:");
        println!("{}", "-".repeat(60));
        println!("{}", template.template);
        println!("{}", "-".repeat(60));
        println!();
        println!("Example Output (with 'cowboy ninja'):");
        println!("{}", "-".repeat(60));
        let example = template.template.replace("${description}", "cowboy ninja");
        println!("{}", example);
        println!("{}", "-".repeat(60));
        println!();
    } else {
        let available: Vec<_> = list_templates();
        anyhow::bail!(
            "Template '{}' not found\n\nAvailable templates:\n{}",
            name,
            available
                .iter()
                .map(|t| format!("  - {}", t))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    Ok(())
}

/// Handle CLI approval prompt for generated image.
fn handle_cli_approval(
    approval_data: &asset_tap_core::types::ApprovalData,
) -> anyhow::Result<asset_tap_core::types::ApprovalResponse> {
    use asset_tap_core::types::ApprovalResponse;
    use std::io::{self, Write};

    println!();
    println!("{}", "=".repeat(60));
    println!("  🖼️  Image Generated - Review Required");
    println!("{}", "=".repeat(60));
    println!();
    println!("  Prompt: {}", approval_data.prompt);
    println!("  Model:  {}", approval_data.model);
    println!("  Image:  {}", approval_data.image_path.display());
    println!();
    println!("  💡 TIP: Open the image in your file browser to review it.");
    println!();
    println!("{}", "-".repeat(60));

    loop {
        print!("  Proceed to 3D generation? [Y/n/r] (Y=yes, n=no, r=regenerate): ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let choice = input.trim().to_lowercase();

        match choice.as_str() {
            "" | "y" | "yes" => {
                println!("  ✓ Approved - Continuing to 3D generation...");
                return Ok(ApprovalResponse::Approve);
            }
            "n" | "no" => {
                println!("  ✗ Cancelled - Image generation stopped.");
                return Ok(ApprovalResponse::Reject);
            }
            "r" | "regenerate" => {
                println!("  ↻ Regenerating image with same prompt...");
                return Ok(ApprovalResponse::Regenerate);
            }
            _ => {
                println!("  ⚠️  Invalid choice. Please enter Y (yes), n (no), or r (regenerate).");
                continue;
            }
        }
    }
}

fn print_progress(progress: &Progress) {
    let display = format_progress(progress);

    // CLI-specific formatting: some updates use carriage return for in-place updates
    match progress {
        Progress::Started { stage, .. } => {
            // Stage start gets its own line with stage-specific icon
            println!("\n{} {}", stage_icon(stage), display.message);
        }
        Progress::Queued { .. } | Progress::Downloading { .. } => {
            // These update in-place with carriage return
            print!("\r   {} {:<40}", display.icon, display.message);
            io::stdout().flush().ok();
        }
        Progress::Processing { message, .. } => {
            match message {
                Some(msg) if msg.contains("elapsed") => {
                    // Periodic elapsed-time updates: overwrite in-place
                    print!("\r   {} {:<60}", display.icon, display.message);
                    io::stdout().flush().ok();
                }
                Some(_) => {
                    // Status change (e.g., "Downloading result..."): new line
                    println!("   {} {}", display.icon, display.message);
                }
                None => {
                    print!("\r   {} {:<60}", display.icon, display.message);
                    io::stdout().flush().ok();
                }
            }
        }
        Progress::Completed { .. } | Progress::Failed { .. } => {
            // Completion and failure get newlines for visibility
            println!("\n   {} {}", display.icon, display.message);
        }
        Progress::Log { .. } => {
            println!("   {} {}", display.icon, display.message);
        }
        Progress::Retrying { .. } => {
            println!("   {} {}", display.icon, display.message);
        }
        Progress::AwaitingApproval { .. } => {
            // Approval required - print message
            println!("\n   {} {}", display.icon, display.message);
        }
    }
}

fn print_summary(output: &asset_tap_core::PipelineOutput) {
    println!();
    println!("{}", "=".repeat(60));
    println!("  ✨ Pipeline Complete!");
    println!("{}", "=".repeat(60));

    if let Some(ref dir) = output.output_dir {
        println!("\n  📁 Output: {}", dir.display());
    }

    if let Some(ref prompt) = output.prompt {
        println!("  📝 Prompt: {}", prompt);
    }

    if let Some(ref path) = output.image_path {
        println!("  🖼️  Image:  {}", path.display());
    } else if let Some(ref url) = output.image_url {
        println!("  🖼️  Image:  {}", url);
    }

    if let Some(ref path) = output.model_path {
        println!("  🧊 GLB:    {}", path.display());
    }

    if let Some(ref path) = output.fbx_path {
        println!("  📦 FBX:    {}", path.display());
    }

    if let Some(ref path) = output.textures_dir {
        println!("  🎨 Textures: {}", path.display());
    }

    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_param_value_parses_as_null() {
        // `--param seed=` should drop the field so the provider's default kicks in.
        let parsed = parse_param_values(&["seed=".to_string()]).unwrap();
        assert_eq!(parsed.get("seed"), Some(&serde_json::Value::Null));
    }

    #[test]
    fn bool_param_parses() {
        let parsed = parse_param_values(&["flag=true".to_string()]).unwrap();
        assert_eq!(parsed.get("flag"), Some(&serde_json::Value::Bool(true)));
    }

    #[test]
    fn numeric_param_parses_as_int_when_possible() {
        let parsed = parse_param_values(&["count=42".to_string()]).unwrap();
        assert_eq!(parsed.get("count"), Some(&serde_json::json!(42)));
    }

    fn mk_def(
        ty: ParameterType,
        options: Option<Vec<serde_json::Value>>,
    ) -> asset_tap_core::providers::ParameterDef {
        asset_tap_core::providers::ParameterDef {
            name: "x".into(),
            label: "x".into(),
            description: None,
            param_type: ty,
            default: serde_json::json!(null),
            min: None,
            max: None,
            step: None,
            options,
            widget: None,
            allow_unset: false,
        }
    }

    #[test]
    fn null_coerces_through_any_declared_type() {
        // Null is the "clear" signal and must pass through regardless of type.
        for ty in [
            ParameterType::Integer,
            ParameterType::Float,
            ParameterType::Boolean,
            ParameterType::String,
            ParameterType::Select,
        ] {
            let opts = if matches!(ty, ParameterType::Select) {
                Some(vec![serde_json::json!("a")])
            } else {
                None
            };
            let def = mk_def(ty.clone(), opts);
            let out = coerce_param_value("x", &serde_json::Value::Null, &def).unwrap();
            assert_eq!(out, serde_json::Value::Null, "type {:?} rejected null", ty);
        }
    }

    #[test]
    fn select_accepts_numeric_option_via_string_form() {
        // --param resolution=512 parses as integer; the option list has [512, 1024, 1536]
        // as numbers, so exact-match works. This covers the common case.
        let def = mk_def(
            ParameterType::Select,
            Some(vec![
                serde_json::json!(512),
                serde_json::json!(1024),
                serde_json::json!(1536),
            ]),
        );
        let result = coerce_param_value("resolution", &serde_json::json!(512), &def).unwrap();
        assert_eq!(result, serde_json::json!(512));
    }

    #[test]
    fn select_coerces_string_input_to_numeric_option() {
        // If a user explicitly passes --param foo=512 and options are strings
        // ["512", "1024"], we should coerce to the string form.
        let def = mk_def(
            ParameterType::Select,
            Some(vec![serde_json::json!("512"), serde_json::json!("1024")]),
        );
        // `--param foo=512` would parse as integer 512; should match the "512" string option.
        let result = coerce_param_value("foo", &serde_json::json!(512), &def).unwrap();
        assert_eq!(result, serde_json::json!("512"));
    }

    #[test]
    fn select_rejects_value_not_in_options() {
        let def = mk_def(
            ParameterType::Select,
            Some(vec![serde_json::json!("a"), serde_json::json!("b")]),
        );
        assert!(coerce_param_value("x", &serde_json::json!("c"), &def).is_err());
    }

    fn mk_model(id: &str, params: &[&str]) -> ModelInfo {
        ModelInfo {
            id: id.into(),
            name: id.into(),
            description: None,
            is_default: false,
            endpoint: String::new(),
            metadata: None,
            parameters: params
                .iter()
                .map(|name| asset_tap_core::providers::ParameterDef {
                    name: (*name).into(),
                    ..mk_def(ParameterType::String, None)
                })
                .collect(),
        }
    }

    #[test]
    fn image_only_run_ignores_3d_parameters() {
        // The 3D stage is skipped, so its knobs are neither accepted nor
        // advertised.
        let active = ActiveModels {
            image: StageModel::Active(Box::new(mk_model(
                "meshy/nano-banana-pro",
                &["aspect_ratio"],
            ))),
            model_3d: StageModel::Skipped,
        };

        let ok = route_params(
            &HashMap::from([("aspect_ratio".to_string(), serde_json::json!("1:1"))]),
            &active,
        )
        .expect("image param must be accepted under --image-only");
        assert_eq!(
            ok.image.get("aspect_ratio"),
            Some(&serde_json::json!("1:1"))
        );
        assert!(ok.model_3d.is_empty());

        let err = route_params(
            &HashMap::from([("topology".to_string(), serde_json::json!("quad"))]),
            &active,
        )
        .expect_err("3D param must not be accepted under --image-only");
        let msg = err.to_string();
        assert!(msg.contains("meshy/nano-banana-pro"), "{msg}");
        assert!(msg.contains("aspect_ratio"), "{msg}");
        assert!(msg.contains("image-only"), "{msg}");
    }

    #[test]
    fn unknown_param_lists_only_active_models() {
        let active = ActiveModels {
            image: StageModel::Active(Box::new(mk_model("img-model", &["aspect_ratio"]))),
            model_3d: StageModel::Active(Box::new(mk_model("3d-model", &["topology"]))),
        };
        let err = route_params(
            &HashMap::from([("output_format".to_string(), serde_json::json!("png"))]),
            &active,
        )
        .expect_err("unknown param must be rejected");
        let msg = err.to_string();
        assert!(msg.contains("img-model (text-to-image)"), "{msg}");
        assert!(msg.contains("3d-model (image-to-3D)"), "{msg}");
    }

    #[test]
    fn param_failures_are_usage_errors() {
        // Spec §2: a mistyped invocation exits 2, not 1, so a consumer can
        // tell an invalid command from a failed run.
        let active = ActiveModels {
            image: StageModel::Active(Box::new(mk_model("img-model", &["aspect_ratio"]))),
            model_3d: StageModel::Skipped,
        };
        let err = route_params(
            &HashMap::from([("nope".to_string(), serde_json::json!(1))]),
            &active,
        )
        .unwrap_err();
        assert!(machine::find_usage_error(&err).is_some());
    }

    #[test]
    fn unresolved_model_is_not_reported_as_a_skipped_stage() {
        // An unresolved 3D model leaves the same "no model here" hole as
        // --image-only but means something different, so the note must not
        // report a flag the user never passed.
        let active = ActiveModels {
            image: StageModel::Active(Box::new(mk_model("img-model", &["aspect_ratio"]))),
            model_3d: StageModel::Unresolved(Some("does-not-exist".into())),
        };
        let err = route_params(
            &HashMap::from([("topology".to_string(), serde_json::json!("quad"))]),
            &active,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(
            !msg.contains("image-only"),
            "unresolvable model must not be reported as --image-only: {msg}"
        );
        assert!(msg.contains("does-not-exist"), "{msg}");
    }

    #[test]
    fn no_provider_configured_produces_no_misleading_note() {
        // Nothing named and nothing resolved: the cause is unknown, so the
        // message states no cause.
        let active = ActiveModels {
            image: StageModel::Active(Box::new(mk_model("img-model", &["aspect_ratio"]))),
            model_3d: StageModel::Unresolved(None),
        };
        let err = route_params(
            &HashMap::from([("topology".to_string(), serde_json::json!("quad"))]),
            &active,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(!msg.contains("Note:"), "unexpected note: {msg}");
    }
}
