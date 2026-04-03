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
    providers::{ParameterType, ProviderCapability, ProviderRegistry},
    settings::{get_output_dir, is_dev_mode},
    templates::{apply_template, list_templates},
    types::Progress,
};

use clap::Parser;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use walkdir::WalkDir;

/// Asset Tap - Generate 3D models from text prompts
#[derive(Parser)]
#[command(name = "asset-tap")]
#[command(about = "Asset Tap - AI-powered text-to-3D generation")]
#[command(version)]
struct Cli {
    /// Text prompt describing what to create (interactive if not provided)
    prompt: Option<String>,

    /// Auto-confirm all prompts (non-interactive mode)
    #[arg(short = 'y', long)]
    yes: bool,

    /// Skip FBX conversion (GLB only)
    #[arg(long)]
    no_fbx: bool,

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
}

/// Print ASCII art banner
fn print_banner() {
    println!(concat!(
        "\n",
        "   ___                _     _____\n",
        "  / _ |___ ___ ___ __| |_  /__   \\__ _ _ __  _ __\n",
        " / /_\\/ __/ __|/ _ \\ __| __| / /\\/ _` | '_ \\| '_ \\\n",
        "/ /_\\\\__ \\__ \\  __/ |_| |_  / / | (_| | |_) | |_) |\n",
        "\\____/___/___/\\___|\\__|\\__| \\/   \\__,_| .__/| .__/\n",
        "                                      |_|   |_|\n",
    ));
}

fn main() -> anyhow::Result<()> {
    // Load .env file (before tokio runtime starts, so set_var is safe)
    dotenvy::dotenv().ok();

    let cli = Cli::parse();

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
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async_main(cli))
}

async fn async_main(cli: Cli) -> anyhow::Result<()> {
    // Initialize tracing with dual output: console + rolling log file
    let _guard = asset_tap_core::error_log::init_tracing();

    // Show banner for main commands (not for --list or --inspect)
    if !cli.list && !cli.list_providers && cli.inspect_template.is_none() && !cli.convert_webp {
        print_banner();
    }

    // Handle --inspect-template flag (no registry needed)
    if let Some(template_name) = &cli.inspect_template {
        return handle_inspect_template(template_name);
    }

    // Handle --convert-webp flag (no registry needed)
    if cli.convert_webp {
        return handle_convert_webp(&cli.output);
    }

    // Handle --export-bundle flag (no registry needed)
    if let Some(ref bundle_dir) = cli.export_bundle {
        return handle_export_bundle(bundle_dir, &cli.output, cli.name.as_deref());
    }

    // Handle --convert-fbx flag (no registry needed)
    if let Some(ref path) = cli.convert_fbx {
        return handle_convert_fbx(path);
    }

    // Handle mock mode
    #[cfg(feature = "mock")]
    if cli.mock {
        println!(
            "🎭 Running in mock mode{}",
            if cli.mock_delay { " (with delays)" } else { "" }
        );
    }

    // Create provider registry once and reuse everywhere
    let registry = ProviderRegistry::new();

    // Handle --list-providers flag
    if cli.list_providers {
        print_available_providers(&registry);
        return Ok(());
    }

    // Handle --list flag
    if cli.list {
        print_available_options(&registry);
        return Ok(());
    }

    // Show dev mode indicator
    if is_dev_mode() {
        println!("🔧 Running in development mode (using ./output/)");
    }

    // Handle --convert-only mode
    if cli.convert_only {
        return handle_convert_only(!cli.no_fbx);
    }

    // Load settings
    use asset_tap_core::settings::Settings;
    let settings = Settings::load();

    // Build pipeline configuration
    let mut config = build_config(&cli, &settings)?;

    // Validate required settings
    validate_requirements(&config, &settings, &registry)?;

    // Parse and validate --param overrides
    if !cli.params.is_empty() {
        let parsed = parse_param_values(&cli.params)?;

        // Resolve effective model IDs for validation
        let effective_image_model = config
            .image_model
            .clone()
            .or_else(|| get_default_text_to_image_model(&registry));
        let effective_3d_model = if config.model_3d.is_empty() {
            get_default_image_to_3d_model(&registry).unwrap_or_default()
        } else {
            config.model_3d.clone()
        };

        let (image_params, model_3d_params) = route_params(
            &parsed,
            &registry,
            effective_image_model.as_deref(),
            &effective_3d_model,
        )?;

        if !image_params.is_empty() {
            config = config.with_image_model_params(image_params);
        }
        if !model_3d_params.is_empty() {
            config = config.with_3d_model_params(model_3d_params);
        }
    }

    // Enable approval if: --approve flag OR settings require it (but not in auto-confirm mode)
    if (cli.approve || settings.require_image_approval) && !cli.yes {
        config = config.with_image_approval();
    }

    // Run the pipeline
    let (mut progress_rx, handle, approval_tx, _cancel_tx) =
        run_pipeline(config.clone(), &registry).await?;

    // Process progress updates
    while let Some(progress) = progress_rx.recv().await {
        // Handle approval requests in CLI
        if let asset_tap_core::types::Progress::AwaitingApproval { approval_data, .. } = &progress {
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

    // Print summary
    print_summary(&output);

    Ok(())
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
    expected: &ParameterType,
) -> anyhow::Result<serde_json::Value> {
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
        ParameterType::String | ParameterType::Select => match value {
            serde_json::Value::String(_) => Ok(value.clone()),
            _ => anyhow::bail!("Parameter '{}' expects a string, got '{}'", key, value),
        },
    }
}

/// Validate, coerce, and route parsed parameters to image and/or 3D models.
///
/// Each parameter must be declared by at least one of the two active models.
/// Values are coerced to match the declared type (e.g., integer → float).
/// Returns `(image_params, model_3d_params)`.
fn route_params(
    params: &HashMap<String, serde_json::Value>,
    registry: &ProviderRegistry,
    image_model_id: Option<&str>,
    model_3d_id: &str,
) -> anyhow::Result<(
    HashMap<String, serde_json::Value>,
    HashMap<String, serde_json::Value>,
)> {
    if params.is_empty() {
        return Ok((HashMap::new(), HashMap::new()));
    }

    let providers = registry.list_all();

    // Look up full model info (with parameter defs) for each active model
    let image_model =
        image_model_id.and_then(|id| providers.iter().find_map(|p| p.get_model(id).ok()));

    let model_3d = if model_3d_id.is_empty() {
        None
    } else {
        providers.iter().find_map(|p| p.get_model(model_3d_id).ok())
    };

    // Build name → ParameterDef lookup for each model
    let image_param_defs: HashMap<&str, &asset_tap_core::providers::ParameterDef> = image_model
        .as_ref()
        .map(|m| m.parameters.iter().map(|p| (p.name.as_str(), p)).collect())
        .unwrap_or_default();

    let model_3d_param_defs: HashMap<&str, &asset_tap_core::providers::ParameterDef> = model_3d
        .as_ref()
        .map(|m| m.parameters.iter().map(|p| (p.name.as_str(), p)).collect())
        .unwrap_or_default();

    let mut image_params = HashMap::new();
    let mut model_3d_params = HashMap::new();

    for (key, value) in params {
        let in_image = image_param_defs.get(key.as_str());
        let in_3d = model_3d_param_defs.get(key.as_str());

        match (in_image, in_3d) {
            (Some(def), None) => {
                let coerced = coerce_param_value(key, value, &def.param_type)?;
                image_params.insert(key.clone(), coerced);
            }
            (None, Some(def)) => {
                let coerced = coerce_param_value(key, value, &def.param_type)?;
                model_3d_params.insert(key.clone(), coerced);
            }
            (Some(_), Some(def)) => {
                eprintln!(
                    "  ⚠️  Parameter '{}' is declared by both image and 3D models; routing to 3D model",
                    key
                );
                let coerced = coerce_param_value(key, value, &def.param_type)?;
                model_3d_params.insert(key.clone(), coerced);
            }
            (None, None) => {
                let mut valid: Vec<&str> = image_param_defs
                    .keys()
                    .chain(model_3d_param_defs.keys())
                    .copied()
                    .collect();
                valid.sort();
                valid.dedup();

                let valid_list = if valid.is_empty() {
                    "  (none — selected models have no tunable parameters)".to_string()
                } else {
                    valid
                        .iter()
                        .map(|p| format!("  - {}", p))
                        .collect::<Vec<_>>()
                        .join("\n")
                };

                anyhow::bail!(
                    "Unknown parameter '{}' for the selected models.\n\n\
                     Valid parameters:\n{}",
                    key,
                    valid_list
                );
            }
        }
    }

    Ok((image_params, model_3d_params))
}

fn build_config(
    cli: &Cli,
    settings: &asset_tap_core::settings::Settings,
) -> anyhow::Result<PipelineConfig> {
    // Get user input and expand template if specified
    let user_input = match (&cli.prompt, &cli.template) {
        (Some(p), _) => p.trim().to_string(),
        (None, _) if cli.image.is_some() => String::new(),
        (None, _) if cli.yes => {
            anyhow::bail!("Prompt required in non-interactive mode (-y)")
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

    // Apply custom Blender path from settings
    if let Some(ref blender) = settings.blender_path
        && !blender.is_empty()
    {
        config = config.with_blender_path(blender);
    }

    Ok(config)
}

fn validate_requirements(
    config: &PipelineConfig,
    settings: &asset_tap_core::settings::Settings,
    registry: &ProviderRegistry,
) -> anyhow::Result<()> {
    // Validate output directory is set
    if config.output_dir.is_none() {
        anyhow::bail!(
            "Output directory is required. Set it via:\n\
            1. --output flag: asset-tap --output /path/to/output \"prompt\"\n\
            2. Settings file (GUI): Configure in the application settings\n\
            3. Dev mode: Uses ./output/ by default"
        );
    }

    // Validate output directory is not empty
    if let Some(ref dir) = config.output_dir
        && dir.as_os_str().is_empty()
    {
        anyhow::bail!("Output directory cannot be empty");
    }

    // Validate API key is available (check environment) - skip in mock mode
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
        anyhow::bail!(
            "API key(s) required: {env_list}\n\
            Set via:\n\
            1. Environment variable (e.g., {env_var}=your_key_here)\n\
            2. .env file\n\
            3. Settings file (GUI): Configure in the application settings{url_hint}",
            env_var = env_vars.first().unwrap_or(&"API_KEY".to_string()),
        );
    }

    Ok(())
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
    println!();
    println!("Available Providers");
    println!("{}", "=".repeat(60));
    let providers = registry.list_available();

    if providers.is_empty() {
        println!("\n⚠️  No providers available");
        println!("   Configure API key(s) in environment variables.");
        // List all providers and their required env vars
        for provider in &registry.list_all() {
            let meta = provider.metadata();
            if !meta.required_env_vars.is_empty() {
                println!(
                    "   - {} for {}",
                    meta.required_env_vars.join(", "),
                    meta.name
                );
            }
        }
        println!();
        return;
    }

    for provider in &providers {
        let metadata = provider.metadata();
        println!("\n{} - {}", metadata.name, metadata.description);
        println!("  ID: {} (-p {})", metadata.id, metadata.id);

        if !metadata.required_env_vars.is_empty() {
            println!("  Env: {}", metadata.required_env_vars.join(", "));
        }

        // List text-to-image models
        let text_to_image = provider.list_models(ProviderCapability::TextToImage);
        if !text_to_image.is_empty() {
            println!("\n  Text-to-Image Models (--image-model):");
            for model in &text_to_image {
                let default_marker = if model.is_default { " (default)" } else { "" };
                let desc = model.description.as_deref().unwrap_or("");
                println!("    • {} - {}{}", model.id, desc, default_marker);
            }
        }

        // List image-to-3D models
        let image_to_3d = provider.list_models(ProviderCapability::ImageTo3D);
        if !image_to_3d.is_empty() {
            println!("\n  Image-to-3D Models (--3d-model):");
            for model in &image_to_3d {
                let default_marker = if model.is_default { " (default)" } else { "" };
                let desc = model.description.as_deref().unwrap_or("");
                println!("    • {} - {}{}", model.id, desc, default_marker);
            }
        }
    }

    println!();
}

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
