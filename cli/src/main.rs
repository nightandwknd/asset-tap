//! Asset Tap CLI
//!
//! Open-source game asset generation pipeline.

use asset_tap_core::constants::files::bundle as bundle_files;
#[cfg(feature = "mock")]
use asset_tap_core::constants::http::env;
use asset_tap_core::{
    config::{
        get_default_image_to_3d_model, get_default_text_to_image_model, list_image_to_3d_models,
        list_text_to_image_models,
    },
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

mod mcp;

/// Trailing help block (spec §7 Agent ergonomics). The `--mock` example is
/// only shown in builds that have the flag: an agent reading release `--help`
/// must never be pointed at an argument the binary rejects.
const AFTER_HELP: &str = concat!(
    "EXAMPLES — 2D (image only):\n",
    "  asset-tap --image-only --param aspect_ratio=1:1 \"cobblestone\"\n",
    "                                               square image, no 3D stage\n",
    "  asset-tap --image-only --install sprites/idle.png \"a goblin archer\"\n",
    "                                               copied where you want it\n",
    "  asset-tap --image-only --image-model meshy/gpt-image-2 --param aspect_ratio=3:2 \"a banner\"\n",
    "                                               pick the image model and its knobs\n",
    "\n",
    "EXAMPLES — 3D:\n",
    "  asset-tap \"a stylized sci-fi crate\"          prompt to GLB\n",
    "  asset-tap --image ref.png                    image-to-3D from an existing image\n",
    "  asset-tap \"a crate\" --install models/crate.glb\n",
    "                                               also copy the GLB into your project\n",
    "  asset-tap --rig --clip walk -y \"a knight\"    generate, bind, apply walk\n",
    "  asset-tap bind --mesh model.glb --clip walk  bind an existing mesh\n",
    "\n",
    "EXAMPLES — tooling:\n",
    "  asset-tap \"a crate\" --json -o ./out          programmatic use: parse NDJSON events\n",
    "  asset-tap --list --json                      machine-readable model/template catalog\n",
    "  asset-tap auth list --json                   which providers have a key (preflight)\n",
    mock_example!(),
    "  echo $KEY | asset-tap auth set fal.ai        store a provider API key\n",
    "  asset-tap demo download                      fetch the showcase demo bundle\n",
    "  asset-tap clip download                      fetch the free Standard clip packs\n",
    "  asset-tap clip install --from DIR            install a Quaternius zip you already have\n",
    "\n",
    "CONCURRENCY:\n",
    "  Providers rate-limit per API key. A 429 (or 5xx) while polling is retried with\n",
    "  exponential backoff (2s doubling to a 30s cap, up to 5 consecutive failures);\n",
    "  Meshy documents no safe parallelism, so run its jobs one at a time.\n",
    "\n",
    "AUTHENTICATION:\n",
    "  Provider keys resolve from stored settings first, then environment variables\n",
    "  (e.g. FAL_KEY). `asset-tap auth list` shows each provider's effective source.\n",
    "\n",
    "EXIT CODES:\n",
    "  0 ok · 1 other error · 2 usage · 3 auth/key · 4 provider · 5 canceled ·\n",
    "  6 network/timeout · 7 local environment (filesystem)\n",
    "\n",
    "For the full machine interface (NDJSON events, result contract, catalog schema),\n",
    "run: asset-tap --machine-help",
);

#[derive(Parser)]
#[command(name = "asset-tap")]
// `about` only, deliberately: clap prints `long_about` above the usage line on
// `--help`, and APP_DESCRIPTION is a paragraph. The long form lives in
// `--machine-help` and the MCP server instructions instead.
#[command(about = asset_tap_core::constants::files::APP_CATEGORY)]
#[command(version)]
#[command(after_help = AFTER_HELP)]
struct Cli {
    /// Text prompt describing what to create (interactive if not provided)
    prompt: Option<String>,

    /// Auto-confirm the image approval step (skips the y/n/r prompt after image generation)
    #[arg(short = 'y', long)]
    yes: bool,

    /// Stop after image generation: an image-only bundle with no 3D model
    #[arg(long)]
    image_only: bool,

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

    /// Dump a provider's raw resolved YAML config as JSON (anchors expanded).
    ///
    /// Hidden: this is a tooling hook (scripts/audit-fal-schemas.sh) rather
    /// than a user-facing command, so the same parser the app runs on is the
    /// one auditing scripts read, instead of a second hand-rolled YAML reader.
    #[arg(long, value_name = "PROVIDER_ID", hide = true)]
    dump_provider_config: Option<String>,

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

    /// Copy the run's primary artifact to PATH when it finishes
    /// (`model.glb`, or `image.png` under --image-only).
    ///
    /// A PATH ending in `.glb`/`.png` is the exact destination file (parent
    /// directories are created). Anything else — an existing directory, a
    /// trailing separator, or no extension at all — is a directory and
    /// receives `<--name, else the bundle folder>.<ext>`. An extension that
    /// doesn't match the artifact this run produces is a usage error, raised
    /// before generation starts. The bundle itself is still written as
    /// normal; this is a copy.
    #[arg(
        long,
        value_name = "PATH",
        conflicts_with_all = ["convert_webp", "export_bundle", "inspect_template"]
    )]
    install: Option<PathBuf>,

    /// Export a bundle directory as a zip archive (requires --name if bundle is unnamed)
    #[arg(long, value_name = "BUNDLE_DIR")]
    export_bundle: Option<PathBuf>,

    /// Bind the mesh to the shipped humanoid armature
    #[arg(long, conflicts_with = "image_only")]
    rig: bool,

    /// Clip to bake after rigging (`walk`, `run`, `idle`, or a pack animation
    /// name). Repeat for several: the model gets one animation per clip, as
    /// Mixamo and Meshy do, rather than one export per clip.
    #[arg(long, value_name = "NAME", conflicts_with = "image_only")]
    clip: Vec<String>,

    /// Clip-pack directory (overrides the installed pack)
    #[arg(long, value_name = "DIR")]
    clip_pack: Option<PathBuf>,

    /// Set model parameter overrides (repeatable, e.g. --param guidance_scale=7.0 --param topology=quad)
    #[arg(long = "param", value_name = "KEY=VALUE")]
    params: Vec<String>,

    /// Emit machine-readable NDJSON events on stdout (implies --yes; run --machine-help for the full contract)
    ///
    /// Contract: stdout carries NDJSON only, one JSON object per line
    /// (`start`, `progress`, `log`, then exactly one `result`). All
    /// human-facing diagnostics go to stderr; never parse stderr. Implies
    /// --yes (fully non-interactive). Exit codes: 0 ok, 2 usage, 3 auth/key,
    /// 4 provider, 5 canceled, 6 network, 7 local environment, 1 other.
    /// Full spec (event fields, result shape, catalog schema): --machine-help
    #[arg(
        long,
        conflicts_with_all = [
            "approve",
            "convert_webp",
            "export_bundle",
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
    /// Serve the Model Context Protocol over stdio (for MCP hosts: Claude
    /// Desktop, Cursor, IDE agents). Same internals as the CLI: the tools
    /// are `list_catalog`, `auth_status`, `inspect_bundle`, `clip_download`,
    /// `generate`. Add with e.g. `claude mcp add asset-tap -- asset-tap mcp`.
    Mcp,
    /// Rig a mesh and bake clips into it.
    ///
    /// For machine-readable output the flag precedes the subcommand:
    /// `asset-tap --json bind --mesh model.glb --clip walk`.
    Bind {
        /// Mesh GLB (or a bundle directory containing model.glb)
        #[arg(long, value_name = "PATH")]
        mesh: PathBuf,
        /// Clip to bake in (`walk`, `Walk_Loop`, …). Repeat for several:
        /// the model gets one animation per clip, as Mixamo and Meshy do,
        /// rather than one export per clip. Defaults to `walk` when omitted;
        /// use --fit-only for a rig with no animation.
        #[arg(long, value_name = "NAME")]
        clip: Vec<String>,
        /// Fit and weight only: a skinned T-pose, no animation (without
        /// this, no --clip means `walk`)
        #[arg(long)]
        fit_only: bool,
        /// Re-fit even if the mesh is already rigged, discarding its pose.
        ///
        /// By default a rigged mesh keeps its skeleton and weights, so adding
        /// a clip cannot silently undo joints you arranged by hand.
        #[arg(long)]
        refit: bool,
        /// Output GLB (default: overwrite the mesh, or write next to a bundle)
        #[arg(short, long, value_name = "PATH")]
        output: Option<PathBuf>,
        /// Clip-pack directory (overrides the installed pack; same flag as
        /// the root `--clip-pack`)
        #[arg(long, value_name = "DIR", alias = "pack")]
        clip_pack: Option<PathBuf>,
    },
    /// Manage humanoid clip packs
    Clip {
        #[command(subcommand)]
        action: ClipAction,
    },
}

#[derive(Subcommand)]
enum ClipAction {
    /// Download the free Standard packs from the latest GitHub Release
    /// (hash-verified). Already-installed ids are left alone, so a Source
    /// upgrade is never overwritten. `ASSET_TAP_CLIP_PACKS_DIR` skips the
    /// network (used by tests and a source checkout).
    Download {
        /// Replace Standard packs previously installed by `clip download`.
        /// Never overwrites a pack from `clip install` (Source or custom).
        #[arg(long)]
        force: bool,
    },
    /// Install an animation pack from a Quaternius download
    Install {
        /// `.zip`, extracted folder, or glTF/GLB. A directory is searched
        /// for the animation library, ignoring mannequin meshes and `_RM`
        /// root-motion variants. Packs:
        /// https://quaternius.com/packs/universalanimationlibrary.html
        /// https://quaternius.com/packs/universalanimationlibrary2.html
        #[arg(long, value_name = "PATH")]
        from: PathBuf,
        /// Pack id (default: derived from the file name, e.g. `ual1`)
        #[arg(long, value_name = "ID")]
        id: Option<String>,
    },
    /// List the clips available from every installed pack
    List {
        /// Also mark which clips are baked into this model
        #[arg(long, value_name = "PATH")]
        model: Option<PathBuf>,
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
        /// Key material is never included, only whether a key is present and where it comes from.
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
    println!("  {}\n", asset_tap_core::constants::files::APP_HERO);
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
            let wire = machine::classify_error(&err);
            if wire.kind == machine::KIND_UNKNOWN {
                // Nothing recognized it, so the cause chain is all we have.
                eprintln!("Error: {:?}", err);
            } else {
                eprintln!("Error: {}", wire.message);
            }
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
                machine::exit_code_for_kind(wire.kind)
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
        Some(Command::Auth { .. })
            | Some(Command::Demo { .. })
            | Some(Command::Clip { .. })
            | Some(Command::Bind { .. })
            | Some(Command::Mcp)
    );
    let _guard = asset_tap_core::error_log::init_tracing(quiet_console);

    // MCP server: stdout is the JSON-RPC transport from here on — nothing else
    // in this process may print to it (tracing already goes to stderr).
    if let Some(Command::Mcp) = cli.command {
        if cli.json {
            eprintln!("error: '--json' cannot be used with the 'mcp' subcommand");
            return Ok(ExitCode::from(machine::EXIT_USAGE));
        }
        return mcp::serve_stdio().await.map(|_| ExitCode::SUCCESS);
    }

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

    if let Some(Command::Clip { action }) = cli.command {
        return handle_clip(action, cli.json).await;
    }

    if let Some(Command::Bind {
        mesh,
        clip,
        fit_only,
        refit,
        output,
        clip_pack,
    }) = cli.command
    {
        return handle_bind(mesh, clip, fit_only, refit, output, clip_pack, cli.json);
    }

    // Show banner for main commands (not for --list, --inspect, or --json,
    // where stdout must stay machine-readable)
    if !cli.list
        && !cli.list_providers
        && cli.dump_provider_config.is_none()
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

    // Handle --dump-provider-config
    if let Some(provider_id) = &cli.dump_provider_config {
        return handle_dump_provider_config(&registry, provider_id).map(|_| ExitCode::SUCCESS);
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

    // `--install` path shape is a usage error too: check it before `start`.
    validate_install_path(&cli)?;

    if cli.json {
        // --json is non-interactive: a prompt (or --image) must come from the
        // args. This is a usage error, so it exits 2 before the start event.
        if let Err(msg) = non_interactive_input_check(&cli) {
            eprintln!("error: {msg}");
            return Ok(ExitCode::from(machine::EXIT_USAGE));
        }

        machine::emit(&machine::Event::start());
        let run_started = std::time::Instant::now();
        let mut last_stage = None;
        return Ok(
            match run_generation(
                &cli,
                &settings,
                &registry,
                params,
                &mut last_stage,
                RunSink::Cli,
            )
            .await
            {
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

    run_generation(&cli, &settings, &registry, params, &mut None, RunSink::Cli).await?;
    Ok(ExitCode::SUCCESS)
}

/// Run the full generation flow: validate keys and config, execute the
/// pipeline, relay progress (human print or NDJSON), and apply `--name`.
///
/// `last_stage` is updated as stages start so callers can attach stage
/// context to error/cancel results.
/// Non-interactive runs (`--json`, and embedded hosts) can't prompt for input:
/// a prompt or `--image` must come from the arguments. Shared by the CLI's
/// `--json` path and the MCP server so both report the same usage error.
pub(crate) fn non_interactive_input_check(cli: &Cli) -> Result<(), &'static str> {
    if cli.prompt.is_none() && cli.image.is_none() {
        return Err(
            "'--json' requires a prompt argument or '--image' (interactive prompting is disabled)",
        );
    }
    Ok(())
}

/// Where a run's progress goes and who can cancel it.
///
/// `Cli` is the interactive/`--json` binary path (stdout events or human
/// lines, SIGINT cancels). `Embedded` is for in-process hosts — the MCP
/// server — where stdout belongs to a transport: progress is handed to a
/// callback (the CLI's `--json` semantics apply: fully non-interactive, no
/// approvals, no summary) and cancellation comes from a token, never a signal.
pub(crate) enum RunSink<'a> {
    Cli,
    Embedded {
        on_progress: &'a mut (dyn FnMut(&asset_tap_core::types::Progress) + Send),
        cancel: tokio_util::sync::CancellationToken,
    },
}

async fn run_generation(
    cli: &Cli,
    settings: &asset_tap_core::settings::Settings,
    registry: &ProviderRegistry,
    params: ParamOverrides,
    last_stage: &mut Option<asset_tap_core::types::Stage>,
    mut sink: RunSink<'_>,
) -> anyhow::Result<asset_tap_core::PipelineOutput> {
    let embedded = matches!(sink, RunSink::Embedded { .. });
    // Validate API keys before prompting the user for input — otherwise the user
    // types a prompt only to hit a missing-key error with no actionable hint.
    validate_api_keys(settings, registry)?;

    // Build pipeline configuration
    let mut config = build_config(cli)?;

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
    if (cli.approve || settings.require_image_approval)
        && !cli.yes
        && !cli.image_only
        && !cli.json
        && !embedded
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
    match &sink {
        RunSink::Embedded { cancel, .. } => {
            // Embedded: the host's cancellation token drives the pipeline's
            // cancel channel; the process must NOT install signal handlers or
            // exit — it's serving other requests.
            let cancel = cancel.clone();
            let cancel_requested = cancel_requested.clone();
            let cancel_tx = cancel_tx.clone();
            tokio::spawn(async move {
                cancel.cancelled().await;
                cancel_requested.store(true, std::sync::atomic::Ordering::SeqCst);
                let _ = cancel_tx.send(());
            });
        }
        RunSink::Cli => {
            let force_exit_code = if cli.json {
                machine::EXIT_CANCELED
            } else {
                machine::EXIT_SIGINT_HUMAN
            };
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
        if let RunSink::Embedded { on_progress, .. } = &mut sink {
            on_progress(&progress);
        } else if cli.json {
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

    // Copy the primary artifact to `--install PATH`. Not a wire event: the
    // caller supplied the path, so it already knows where the file landed.
    install_primary_artifact(cli, &output)?;

    // Print summary (human mode only — --json reports via the result event,
    // embedded hosts via the tool result)
    if !cli.json && !embedded {
        print_summary(&output);

        // A custom -o that lands outside the configured library means the GUI
        // won't list this bundle — say so instead of letting the user discover
        // it via a failed hunt (or a zip-import detour).
        if cli.output.is_some()
            && let Some(ref bundle_dir) = output.output_dir
        {
            let library = asset_tap_core::settings::Settings::load().output_dir;
            let canon = |p: &std::path::Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
            if !canon(bundle_dir).starts_with(canon(&library)) {
                println!(
                    "  ℹ️  Saved outside your library ({}). The GUI lists bundles from there.",
                    library.display()
                );
                println!(
                    "     Import it via File → Import Bundle Folder…, or omit -o to land in the library."
                );
                println!();
            }
        }
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
/// Reject a numeric `--param` outside the bounds its model declares.
///
/// The GUI clamps to the same `min`/`max` with a slider, so without this the
/// CLI is the only way to send a value the provider will reject — and it fails
/// mid-run, after a paid stage may already have completed, instead of as the
/// usage error it is. A bound the model leaves unset is not enforced.
fn check_range(
    key: &str,
    value: f64,
    def: &asset_tap_core::providers::ParameterDef,
) -> anyhow::Result<()> {
    // Print the bound the way the YAML declares it: whole numbers without a
    // trailing `.0`, so an integer param reads `minimum 4`, not `minimum 4.0`.
    fn show(bound: f64) -> String {
        if bound.fract() == 0.0 && bound.abs() < 1e15 {
            format!("{}", bound as i64)
        } else {
            format!("{bound}")
        }
    }
    let shown = show(value);
    if let Some(min) = def.min
        && value < min
    {
        anyhow::bail!("{key}={shown} is below the minimum {}", show(min));
    }
    if let Some(max) = def.max
        && value > max
    {
        anyhow::bail!("{key}={shown} is above the maximum {}", show(max));
    }
    Ok(())
}

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
                check_range(key, f, def)?;
                Ok(serde_json::json!(f))
            }
            _ => anyhow::bail!("Parameter '{}' expects a float, got '{}'", key, value),
        },
        ParameterType::Integer => match value {
            serde_json::Value::Number(n) => {
                let i = n.as_i64().ok_or_else(|| {
                    anyhow::anyhow!("Parameter '{}' expects an integer, got '{}'", key, value)
                })?;
                check_range(key, i as f64, def)?;
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

    check_conditions(&active.image, &image_params)?;
    check_conditions(&active.model_3d, &model_3d_params)?;

    Ok(ParamOverrides {
        image: image_params,
        model_3d: model_3d_params,
    })
}

/// Reject `--param` combinations the model declares as impossible, and note
/// the defaults that get dropped as a result.
///
/// A knob whose `requires` isn't met (or whose `conflicts_with` fires) was
/// mistyped, not merely unlucky — Meshy ignores `origin_at` without
/// `auto_size`, and rejects `aspect_ratio` alongside Multi-View outright. So
/// it exits 2 before the run starts rather than failing after a paid stage.
/// Parameters left at their YAML default are simply dropped; core does the
/// dropping, we just tell the user which knobs went quiet.
fn check_conditions(
    stage: &StageModel,
    params: &HashMap<String, serde_json::Value>,
) -> anyhow::Result<()> {
    let Some(model) = stage.model() else {
        return Ok(());
    };
    if model.parameters.is_empty() {
        return Ok(());
    }

    let mut effective: HashMap<String, serde_json::Value> = model
        .parameters
        .iter()
        .map(|p| (p.name.clone(), p.default.clone()))
        .collect();
    for (key, value) in params {
        effective.insert(key.clone(), value.clone());
    }
    let explicit: std::collections::HashSet<String> = params.keys().cloned().collect();

    match asset_tap_core::providers::evaluate_conditions(&model.parameters, &effective, &explicit) {
        Ok(dropped) => {
            for drop in dropped {
                // Reached only when the user passed `--param`, so a dropped
                // knob is a consequence of something they just typed and worth
                // saying out loud. stderr keeps `--json` stdout clean.
                tracing::info!("{} not sent: {}", drop.param, drop.because);
                eprintln!("  ℹ️  {} not sent: {}", drop.param, drop.because);
            }
            Ok(())
        }
        Err(violation) => Err(usage_error(format!(
            "--param {}={} {}",
            violation.param,
            params
                .get(&violation.param)
                .map(json_scalar_to_string)
                .unwrap_or_default(),
            violation.detail
        ))),
    }
}

/// The file extension of the artifact a run produces: `png` under
/// `--image-only`, `glb` otherwise.
fn primary_artifact_ext(image_only: bool) -> &'static str {
    // Derived, not spelled: the standard filenames are the contract, and this
    // must follow if one of them ever changes.
    let standard = if image_only {
        asset_tap_core::constants::files::bundle::IMAGE
    } else {
        asset_tap_core::constants::files::bundle::MODEL_GLB
    };
    std::path::Path::new(standard)
        .extension()
        .and_then(|e| e.to_str())
        .expect("standard bundle filenames have extensions")
}

/// Resolve `--install PATH` to the exact file the primary artifact is copied to.
///
/// - A PATH ending in `.png`/`.glb` is the destination file verbatim.
/// - An existing directory, or a PATH ending in a separator, receives
///   `<stem>.<ext>` where `stem` is `--name` when given, else the bundle
///   directory's own name (filled in later by the caller).
/// - Any other extension is a usage error: silently writing a `.glb` to a path
///   the caller spelled `.png` would corrupt whatever pipeline consumes it.
///
/// `bundle_dir_name` is the basename of the run's output directory; it is only
/// consulted for the directory form.
fn resolve_install_path(
    install: &std::path::Path,
    image_only: bool,
    name: Option<&str>,
    bundle_dir_name: &str,
) -> anyhow::Result<PathBuf> {
    let expected = primary_artifact_ext(image_only);
    let raw = install.to_string_lossy();
    let looks_like_dir = raw.ends_with('/') || raw.ends_with(std::path::MAIN_SEPARATOR);

    if !looks_like_dir
        && !install.is_dir()
        && let Some(ext) = install.extension().and_then(|e| e.to_str())
    {
        let ext = ext.to_ascii_lowercase();
        if ext == expected {
            return Ok(install.to_path_buf());
        }
        if ext == "png" || ext == "glb" {
            let (mode, produced) = if image_only {
                ("--image-only", "image.png")
            } else {
                ("a full run", "model.glb")
            };
            return Err(usage_error(format!(
                "--install path '{}' ends in .{ext}, but {mode} produces {produced}. \
                 Use a .{expected} path, or a directory.",
                install.display()
            )));
        }
        return Err(usage_error(format!(
            "--install path '{}' must end in .{expected} (the artifact this run produces) \
             or name a directory.",
            install.display()
        )));
    }

    // Directory form: derive the file name from --name, else the bundle folder.
    let stem = name
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or(bundle_dir_name);
    let stem = asset_tap_core::constants::files::safe_filename_stem(stem);
    Ok(install.join(format!("{stem}.{expected}")))
}

/// Validate `--install` before any work starts, so a bad path is a usage error
/// (exit 2, before the `start` event) instead of a surprise after a paid run.
fn validate_install_path(cli: &Cli) -> anyhow::Result<()> {
    if let Some(ref path) = cli.install {
        resolve_install_path(path, cli.image_only, cli.name.as_deref(), "bundle")?;
    }
    Ok(())
}

/// Copy the run's primary artifact to `--install PATH`.
///
/// Wire-silent by design: `--json` consumers passed the path, so the `result`
/// event is unchanged and the confirmation goes to stderr like every other
/// human message.
fn install_primary_artifact(
    cli: &Cli,
    output: &asset_tap_core::PipelineOutput,
) -> anyhow::Result<()> {
    let Some(ref install) = cli.install else {
        return Ok(());
    };

    let bundle_dir_name = output
        .output_dir
        .as_deref()
        .and_then(|d| d.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "asset".to_string());

    let dest = resolve_install_path(
        install,
        cli.image_only,
        cli.name.as_deref(),
        &bundle_dir_name,
    )?;

    let source = if cli.image_only {
        output.image_path.as_deref()
    } else {
        output.model_path.as_deref()
    };
    // Every failure below is a local filesystem problem, so it carries the
    // `io` kind (exit 7) rather than falling through to `unknown` (exit 1,
    // which a consumer reads as a retryable internal fault).
    let io_error = |message: String| {
        anyhow::Error::new(machine::KindedError {
            kind: machine::KIND_IO_ERROR,
            message,
        })
    };

    let Some(source) = source else {
        return Err(io_error(format!(
            "--install: the run produced no {}",
            if cli.image_only { "image" } else { "model" }
        )));
    };

    if let Some(parent) = dest.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|e| {
            io_error(format!(
                "--install: could not create {}: {e}",
                parent.display()
            ))
        })?;
    }
    std::fs::copy(source, &dest).map_err(|e| {
        io_error(format!(
            "--install: could not copy {} to {}: {e}",
            source.display(),
            dest.display()
        ))
    })?;
    eprintln!("  📦 Installed: {}", dest.display());
    Ok(())
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

fn build_config(cli: &Cli) -> anyhow::Result<PipelineConfig> {
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

    // `--image-only` is rejected against these by clap itself, so there is no
    // "rig what mesh?" case to handle here.
    if cli.rig || !cli.clip.is_empty() {
        config = config.with_clips(cli.clip.clone());
        if let Some(dir) = cli.clip_pack.clone() {
            config = config.with_clip_pack(dir);
        }
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

async fn handle_clip_download(json: bool, force: bool) -> anyhow::Result<ExitCode> {
    if !json {
        println!("Checking clip packs...");
    }
    match asset_tap_core::download_clip_packs(force, |_progress| {}).await {
        Ok(asset_tap_core::ClipPacksDownloadResult::Downloaded { installed, version }) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&machine::ClipDownloadDocument::success(
                        installed.clone(),
                        false,
                        version,
                    ))?
                );
            } else {
                println!(
                    "✅ Installed {} ({})",
                    installed.join(", "),
                    if version == 0 {
                        "local".to_string()
                    } else {
                        format!("v{version}")
                    }
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        Ok(asset_tap_core::ClipPacksDownloadResult::AlreadyExists { version }) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&machine::ClipDownloadDocument::success(
                        Vec::new(),
                        true,
                        version,
                    ))?
                );
            } else {
                println!("✅ Free Standard clip packs already installed");
            }
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            if json {
                let doc = machine::ErrorDocument::from_clip_download_error(&e);
                println!("{}", serde_json::to_string_pretty(&doc)?);
                return Ok(ExitCode::from(machine::exit_code_for_kind(doc.kind)));
            }
            eprintln!("error: clip download failed: {e:#}");
            Ok(ExitCode::from(machine::exit_code_for_kind(
                machine::clip_download_error_kind(&e),
            )))
        }
    }
}

async fn handle_clip(action: ClipAction, json: bool) -> anyhow::Result<ExitCode> {
    match action {
        ClipAction::Download { force } => handle_clip_download(json, force).await,
        ClipAction::Install { from, id } => {
            if json {
                return Err(machine::UsageError {
                    message: "--json cannot be used with 'clip install'".into(),
                }
                .into());
            }
            let pack = asset_tap_core::install_pack_from(&from, id.as_deref())?;
            println!(
                "Installed {} ({}) with {} clips at {}",
                pack.name,
                pack.id,
                pack.clips.len(),
                pack.gltf_path.display()
            );
            Ok(ExitCode::SUCCESS)
        }
        ClipAction::List { model } => {
            let clips = asset_tap_core::list_clips();
            // What the model already holds, so `bake` is not guesswork.
            let baked = match &model {
                Some(path) => match asset_tap_core::baked_clip_names(path) {
                    Ok(b) => b,
                    // Under --json stdout must carry a document either way,
                    // so an unreadable model is the same error object
                    // `clip download` writes, not an empty stdout.
                    Err(e) if json => {
                        let doc =
                            machine::ErrorDocument::from_wire(machine::classify_bind_error(&e));
                        println!("{}", serde_json::to_string_pretty(&doc)?);
                        return Ok(ExitCode::from(machine::exit_code_for_kind(doc.kind)));
                    }
                    Err(e) => return Err(e.into()),
                },
                None => Vec::new(),
            };
            if json {
                let rows: Vec<serde_json::Value> = clips
                    .iter()
                    .map(|c| {
                        let mut v = serde_json::to_value(c).unwrap_or_default();
                        if model.is_some() {
                            v["baked"] = serde_json::Value::Bool(baked.contains(&c.id));
                        }
                        v
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&rows)?);
            } else if clips.is_empty() {
                println!(
                    "No animation packs installed.\n    \
                     asset-tap clip download\n    \
                     asset-tap clip install --from /path/to/download.zip\n    \
                     {} \n    \
                     {}",
                    asset_tap_core::rig::UAL1_PAGE,
                    asset_tap_core::rig::UAL2_PAGE
                );
            } else {
                for c in &clips {
                    let mark = if model.is_none() {
                        ""
                    } else if baked.contains(&c.id) {
                        " baked"
                    } else {
                        ""
                    };
                    println!("{:<26} {:<26} {}{mark}", c.id, c.name, c.pack_id);
                }
            }
            // Animations in the model that no installed pack provides: a
            // declarative bake cannot re-source these, so say so plainly.
            let orphans: Vec<&String> = baked
                .iter()
                .filter(|b| !clips.iter().any(|c| &c.id == *b))
                .collect();
            if !orphans.is_empty() && !json {
                println!(
                    "\n{} animation(s) in the model come from no installed pack: {}",
                    orphans.len(),
                    orphans
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn handle_bind(
    mesh: PathBuf,
    clip: Vec<String>,
    fit_only: bool,
    refit: bool,
    output: Option<PathBuf>,
    pack: Option<PathBuf>,
    json: bool,
) -> anyhow::Result<ExitCode> {
    let started = std::time::Instant::now();
    if json {
        machine::emit(&machine::Event::start());
    }
    // Machine mode reports through the wire contract only: stdout is the
    // transport, so a stray human line would corrupt the stream, and once
    // `start` is out every failure must end in a `result` (spec §1).
    let fail = |wire: machine::WireError| -> anyhow::Result<ExitCode> {
        if json {
            let code = machine::exit_code_for_kind(wire.kind);
            machine::emit(&machine::Event::result_error(wire, None));
            return Ok(ExitCode::from(code));
        }
        Err(machine::KindedError {
            kind: wire.kind,
            message: wire.message,
        }
        .into())
    };
    let mesh = if mesh.is_dir() {
        let glb = mesh.join(bundle_files::MODEL_GLB);
        if !glb.is_file() {
            return fail(machine::WireError::bare(
                machine::KIND_IO_ERROR,
                format!("no {} in {}", bundle_files::MODEL_GLB, mesh.display()),
            ));
        }
        glb
    } else {
        mesh
    };
    let out = output.unwrap_or_else(|| mesh.clone());
    let clips: Vec<String> = if fit_only {
        Vec::new()
    } else if clip.is_empty() {
        vec!["walk".to_string()]
    } else {
        clip
    };
    let options = asset_tap_core::BindOptions {
        clips,
        pack_dir: pack,
        fit_only,
        refit,
    };
    if !json {
        println!("Binding {} …", mesh.display());
    }
    let report = match asset_tap_core::bind_mesh(&mesh, &out, &options) {
        Ok(r) => r,
        Err(e) if json => return fail(machine::classify_bind_error(&e)),
        Err(e) => return Err(e.into()),
    };

    if json {
        machine::emit(&machine::Event::result_bind_success(
            out.display().to_string(),
            report.joint_count,
            report.vertex_count,
            report.clips.clone(),
            started.elapsed().as_millis() as u64,
        ));
        return Ok(ExitCode::SUCCESS);
    }

    let clips = if report.clips.is_empty() {
        "-".to_string()
    } else {
        report.clips.join(", ")
    };
    println!(
        "Bound {} joints / {} verts, clips [{}], wrote {}",
        report.joint_count,
        report.vertex_count,
        clips,
        out.display()
    );
    Ok(ExitCode::SUCCESS)
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
                    _ => "none".to_string(),
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

/// Print a provider's raw `ProviderConfig` as pretty JSON, with YAML anchors
/// already expanded by serde_yaml. No API key is required: the registry
/// registers unconfigured providers too, and nothing here touches the network.
fn handle_dump_provider_config(
    registry: &ProviderRegistry,
    provider_id: &str,
) -> anyhow::Result<()> {
    let provider = registry.get(provider_id).ok_or_else(|| {
        let mut ids = registry.list_provider_ids();
        ids.sort();
        anyhow::Error::new(machine::UsageError {
            message: format!(
                "Unknown provider '{provider_id}'. Available providers: {}",
                ids.join(", ")
            ),
        })
    })?;

    let dynamic = provider
        .as_any()
        .downcast_ref::<asset_tap_core::providers::DynamicProvider>()
        .ok_or_else(|| {
            anyhow::Error::new(machine::UsageError {
                message: format!("Provider '{provider_id}' has no YAML config to dump"),
            })
        })?;

    println!(
        "{}",
        serde_json::to_string_pretty(&dynamic.config_snapshot())?
    );
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

    if let Some(ref path) = output.textures_dir {
        println!("  🎨 Textures: {}", path.display());
    }

    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_path_with_matching_extension_is_used_verbatim() {
        let glb = resolve_install_path(
            std::path::Path::new("models/crate.glb"),
            false,
            None,
            "2026-01-01_120000",
        )
        .expect("glb for a 3D run");
        assert_eq!(glb, PathBuf::from("models/crate.glb"));

        let png = resolve_install_path(
            std::path::Path::new("sprites/idle.PNG"),
            true,
            None,
            "2026-01-01_120000",
        )
        .expect("png for an image-only run");
        assert_eq!(png, PathBuf::from("sprites/idle.PNG"));
    }

    /// The wrong extension is the whole point of the check: writing GLB bytes
    /// to a path the caller spelled `.png` would corrupt whatever consumes it.
    #[test]
    fn install_path_extension_mismatch_is_a_usage_error() {
        for (path, image_only) in [
            ("out/a.glb", true),
            ("out/a.png", false),
            ("out/a.fbx", false),
        ] {
            let err = resolve_install_path(std::path::Path::new(path), image_only, None, "bundle")
                .expect_err("{path} should be rejected");
            assert!(
                machine::find_usage_error(&err).is_some(),
                "{path}: expected a usage error, got {err:#}"
            );
        }
    }

    #[test]
    fn install_directory_derives_the_file_name() {
        // Trailing separator: a directory even if it doesn't exist yet.
        let derived = resolve_install_path(
            std::path::Path::new("out/assets/"),
            false,
            None,
            "2026-01-01_120000",
        )
        .expect("directory form");
        assert_eq!(derived, PathBuf::from("out/assets/2026-01-01_120000.glb"));

        // --name wins over the bundle folder.
        let named = resolve_install_path(
            std::path::Path::new("out/assets/"),
            true,
            Some("My Robot"),
            "2026-01-01_120000",
        )
        .expect("named directory form");
        assert_eq!(named, PathBuf::from("out/assets/My Robot.png"));

        // An existing directory with no trailing separator is still a directory.
        let dir = tempfile::tempdir().expect("tempdir");
        let into =
            resolve_install_path(dir.path(), false, Some("crate"), "bundle").expect("existing dir");
        assert_eq!(into, dir.path().join("crate.glb"));
    }

    /// An extensionless PATH that doesn't exist yet is the directory form:
    /// `--install out/today` writes `out/today/<name>.glb`. Guessing it was
    /// meant as a file would mean writing a GLB with no extension.
    #[test]
    fn install_extensionless_path_is_a_directory() {
        let out = resolve_install_path(
            std::path::Path::new("out/today"),
            false,
            None,
            "2026-01-01_120000",
        )
        .expect("extensionless");
        assert_eq!(out, PathBuf::from("out/today/2026-01-01_120000.glb"));
    }

    /// A bundle name is free text; it must not be able to steer the copy out
    /// of the directory the user named.
    #[test]
    fn install_name_is_sanitized_into_a_file_name() {
        let out = resolve_install_path(
            std::path::Path::new("out/"),
            false,
            Some("../../etc/passwd"),
            "bundle",
        )
        .expect("sanitized");
        assert_eq!(out, PathBuf::from("out/_.._etc_passwd.glb"));
    }

    #[test]
    fn install_conflicts_with_the_non_run_flags() {
        for other in [
            "--convert-webp",
            "--export-bundle=some/dir",
            "--inspect-template=prop",
        ] {
            assert!(
                Cli::try_parse_from(["asset-tap", "--install", "out.glb", other]).is_err(),
                "{other} should conflict with --install"
            );
        }
        // --json is not a conflict: --install is wire-silent.
        Cli::try_parse_from(["asset-tap", "--json", "--install", "out.glb", "a crate"])
            .expect("--install combines with --json");
    }

    /// `bind --pack DIR` was renamed to `--clip-pack` (the root flag's name);
    /// the old spelling stays as a hidden alias so existing scripts keep
    /// working.
    #[test]
    fn bind_accepts_clip_pack_and_its_pack_alias() {
        for flag in ["--clip-pack", "--pack"] {
            let cli = Cli::try_parse_from(["asset-tap", "bind", "--mesh", "m.glb", flag, "/packs"])
                .unwrap_or_else(|e| panic!("{flag}: {e}"));
            match cli.command {
                Some(Command::Bind { clip_pack, .. }) => {
                    assert_eq!(clip_pack.as_deref(), Some(std::path::Path::new("/packs")));
                }
                _ => panic!("{flag}: expected bind"),
            }
        }
    }

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
            requires: Default::default(),
            conflicts_with: Default::default(),
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

    fn bounded(ty: ParameterType, min: f64, max: f64) -> asset_tap_core::providers::ParameterDef {
        let mut def = mk_def(ty, None);
        def.name = "num_inference_steps".into();
        def.min = Some(min);
        def.max = Some(max);
        def
    }

    /// The GUI's slider can't leave the declared range, so the CLI is the only
    /// way to send an out-of-bounds value. Catch it as a usage error instead of
    /// letting the provider reject it mid-run.
    #[test]
    fn numeric_param_below_minimum_is_rejected() {
        let def = bounded(ParameterType::Integer, 4.0, 50.0);
        let err = coerce_param_value("num_inference_steps", &serde_json::json!(3), &def)
            .expect_err("3 is below the minimum 4");
        let msg = err.to_string();
        assert!(
            msg.contains("num_inference_steps=3") && msg.contains("minimum 4"),
            "message should name the param, value and bound: {msg}"
        );
        // Whole bounds render without a `.0` tail even though they're f64.
        assert!(
            !msg.contains("4.0"),
            "integer bound should print as 4: {msg}"
        );
    }

    #[test]
    fn numeric_param_above_maximum_is_rejected() {
        let def = bounded(ParameterType::Integer, 4.0, 50.0);
        let err = coerce_param_value("num_inference_steps", &serde_json::json!(51), &def)
            .expect_err("51 is above the maximum 50");
        assert!(err.to_string().contains("maximum 50"), "{err}");
    }

    #[test]
    fn numeric_param_inside_range_is_accepted() {
        let int_def = bounded(ParameterType::Integer, 4.0, 50.0);
        for v in [4, 28, 50] {
            assert_eq!(
                coerce_param_value("num_inference_steps", &serde_json::json!(v), &int_def).unwrap(),
                serde_json::json!(v),
                "{v} is within [4, 50]"
            );
        }

        let float_def = bounded(ParameterType::Float, 1.0, 20.0);
        assert_eq!(
            coerce_param_value("guidance_scale", &serde_json::json!(7.5), &float_def).unwrap(),
            serde_json::json!(7.5)
        );
        assert!(coerce_param_value("guidance_scale", &serde_json::json!(0.5), &float_def).is_err());
    }

    /// `--param seed=` means "unset", which has no value to bound-check.
    #[test]
    fn null_param_skips_range_check() {
        let def = bounded(ParameterType::Integer, 4.0, 50.0);
        assert_eq!(
            coerce_param_value("num_inference_steps", &serde_json::Value::Null, &def).unwrap(),
            serde_json::Value::Null
        );
    }

    /// A model that declares no bounds accepts anything its type allows.
    #[test]
    fn unbounded_numeric_param_is_not_range_checked() {
        let def = mk_def(ParameterType::Integer, None);
        assert_eq!(
            coerce_param_value("seed", &serde_json::json!(i64::MAX), &def).unwrap(),
            serde_json::json!(i64::MAX)
        );
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
