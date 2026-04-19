//! Main asset generation pipeline.
//!
//! This module provides the core pipeline execution for generating 3D models
//! from text prompts or images. The pipeline coordinates multiple stages:
//!
//! 1. **Image Generation** - Text → Image (via [`providers`](crate::providers))
//! 2. **3D Generation** - Image → 3D Model (GLB format)
//! 3. **FBX Export** - GLB → FBX (optional, via [`convert`](crate::convert))
//!
//! # Quick Start
//!
//! ```no_run
//! use asset_tap_core::{PipelineConfig, pipeline::run_pipeline, providers::ProviderRegistry};
//!
//! # async fn example() -> anyhow::Result<()> {
//! let config = PipelineConfig::builder()
//!     .with_prompt("a cowboy ninja with a leather duster, bandana mask, and dual katanas on the back");
//!
//! let registry = ProviderRegistry::new();
//! let (mut progress_rx, handle, _approval_tx, _cancel_tx) = run_pipeline(config, &registry).await?;
//!
//! // Monitor progress
//! tokio::spawn(async move {
//!     while let Some(progress) = progress_rx.recv().await {
//!         println!("Progress: {:?}", progress);
//!     }
//! });
//!
//! let output = handle.await??;
//! if let Some(model_path) = output.model_path {
//!     println!("Model saved to: {}", model_path.display());
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # See Also
//!
//! - [`PipelineConfig`] - Configuration builder
//! - [`PipelineOutput`] - Pipeline results
//! - [`Progress`] - Progress tracking
//! - [`ProviderRegistry`] - Provider management

use crate::api::download_file;
use crate::bundle::BundleMetadata;
use crate::config::{create_generation_dir, create_generation_dir_in};
use crate::constants::files::bundle as bundle_files;
use crate::convert::convert_glb_to_fbx;
use crate::error_log::{ConfigSnapshot, ErrorLog, ErrorType};
use crate::history::GenerationConfig;
use crate::providers::{DynamicProvider, Provider, ProviderCapability, ProviderRegistry};
use crate::types::{ApprovalResponse, Error, PipelineOutput, Progress, Result, Stage};
use chrono::Utc;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Configuration for a pipeline run.
#[derive(Debug, Clone, Default)]
pub struct PipelineConfig {
    /// Text prompt describing what to create (after template expansion, if any).
    pub prompt: Option<String>,

    /// Original user input before template expansion.
    pub user_prompt: Option<String>,

    /// Template name used for prompt (if any).
    pub template: Option<String>,

    /// URL or path to an existing image (skips image generation).
    pub image_url: Option<String>,

    /// Provider ID to use for image generation.
    /// If None, uses the first available provider with text-to-image capability.
    pub image_provider: Option<String>,

    /// Image generation model name.
    pub image_model: Option<String>,

    /// Provider ID to use for 3D generation.
    /// If None, uses the first available provider with image-to-3d capability.
    pub model_3d_provider: Option<String>,

    /// 3D generation model name.
    pub model_3d: String,

    /// Whether to export FBX (requires Blender).
    pub export_fbx: bool,

    /// Custom Blender path (overrides auto-detection).
    pub blender_path: Option<String>,

    /// Base output directory for generated assets.
    /// If None, uses the default OUTPUT_DIR.
    pub output_dir: Option<PathBuf>,

    /// Whether to require user approval after image generation before proceeding to 3D.
    pub require_image_approval: bool,

    /// Channel for sending approval responses back to the pipeline.
    /// This is set internally by run_pipeline and should not be set by users.
    #[doc(hidden)]
    pub approval_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::types::ApprovalResponse>>,

    /// User-tuned parameter overrides for the image generation model.
    pub image_model_params: HashMap<String, serde_json::Value>,

    /// User-tuned parameter overrides for the 3D generation model.
    pub model_3d_params: HashMap<String, serde_json::Value>,
}

impl PipelineConfig {
    /// Create a new pipeline configuration builder with defaults.
    ///
    /// This returns a builder that you can chain with methods like
    /// [`with_prompt()`](Self::with_prompt), [`with_template()`](Self::with_template),
    /// [`with_image_model()`](Self::with_image_model), etc.
    ///
    /// # Examples
    ///
    /// Basic configuration with prompt:
    ///
    /// ```
    /// use asset_tap_core::PipelineConfig;
    ///
    /// let config = PipelineConfig::builder()
    ///     .with_prompt("a cowboy ninja with a leather duster, bandana mask, and dual katanas on the back")
    ///     ;
    /// ```
    ///
    /// Using a template:
    ///
    /// ```
    /// use asset_tap_core::PipelineConfig;
    ///
    /// let config = PipelineConfig::builder()
    ///     .with_template("character")
    ///     .with_prompt("a robot warrior")
    ///     ;
    /// ```
    ///
    /// Specifying providers and models (use provider IDs from your providers/*.yaml configs):
    ///
    /// ```
    /// use asset_tap_core::PipelineConfig;
    ///
    /// let config = PipelineConfig::builder()
    ///     .with_prompt("a spaceship")
    ///     .with_image_provider("my-provider")  // Use the 'id' from provider config
    ///     .with_image_model("model-name")      // Use the model 'id' from provider config
    ///     .with_3d_provider("my-provider")
    ///     .with_3d_model("another-model")
    ///     ;
    /// ```
    pub fn builder() -> Self {
        Self::new()
    }

    /// Create a new pipeline configuration with defaults.
    pub fn new() -> Self {
        Self {
            export_fbx: true,
            ..Default::default()
        }
    }

    /// Set the text prompt.
    pub fn with_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = Some(prompt.into());
        self
    }

    /// Set the original user prompt (before template expansion).
    pub fn with_user_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.user_prompt = Some(prompt.into());
        self
    }

    /// Set the template name.
    pub fn with_template(mut self, template: impl Into<String>) -> Self {
        self.template = Some(template.into());
        self
    }

    /// Set the image generation model.
    pub fn with_image_model(mut self, model: impl Into<String>) -> Self {
        self.image_model = Some(model.into());
        self
    }

    /// Set the 3D generation model.
    pub fn with_3d_model(mut self, model: impl Into<String>) -> Self {
        self.model_3d = model.into();
        self
    }

    /// Set the provider for image generation.
    pub fn with_image_provider(mut self, provider: impl Into<String>) -> Self {
        self.image_provider = Some(provider.into());
        self
    }

    /// Set the provider for 3D generation.
    pub fn with_3d_provider(mut self, provider: impl Into<String>) -> Self {
        self.model_3d_provider = Some(provider.into());
        self
    }

    /// Set the same provider for both image and 3D generation (deprecated, use specific methods).
    #[deprecated(note = "Use with_image_provider and with_3d_provider for more flexibility")]
    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        let p = provider.into();
        self.image_provider = Some(p.clone());
        self.model_3d_provider = Some(p);
        self
    }

    /// Set parameter overrides for the image generation model.
    pub fn with_image_model_params(mut self, params: HashMap<String, serde_json::Value>) -> Self {
        self.image_model_params = params;
        self
    }

    /// Set parameter overrides for the 3D generation model.
    pub fn with_3d_model_params(mut self, params: HashMap<String, serde_json::Value>) -> Self {
        self.model_3d_params = params;
        self
    }

    /// Use an existing image instead of generating one.
    pub fn with_existing_image(mut self, url: impl Into<String>) -> Self {
        self.image_url = Some(url.into());
        self
    }

    /// Disable FBX export.
    pub fn without_fbx(mut self) -> Self {
        self.export_fbx = false;
        self
    }

    /// Set the output directory for generated assets.
    pub fn with_output_dir(mut self, dir: PathBuf) -> Self {
        self.output_dir = Some(dir);
        self
    }

    /// Set a custom Blender path for FBX conversion.
    pub fn with_blender_path(mut self, path: impl Into<String>) -> Self {
        self.blender_path = Some(path.into());
        self
    }

    /// Enable image approval requirement.
    pub fn with_image_approval(mut self) -> Self {
        self.require_image_approval = true;
        self
    }

    /// Determine the effective image model to use.
    pub fn effective_image_model(&self) -> Option<&str> {
        if self.image_url.is_some() {
            return None; // No image generation needed
        }

        // Use user-specified model if provided
        // If None, the provider will use its default model
        self.image_model.as_deref()
    }
}

/// Run the asset generation pipeline with provider support.
///
/// This is the modern pipeline implementation that uses the provider registry.
/// It supports multiple providers through a common interface.
///
/// # Arguments
///
/// * `config` - Pipeline configuration created with [`PipelineConfig::builder()`]
/// * `registry` - Provider registry (use [`ProviderRegistry::new()`] for defaults)
///
/// # Returns
///
/// Returns a tuple of:
/// - [`tokio::sync::mpsc::UnboundedReceiver<Progress>`] - Channel for monitoring progress
/// - [`tokio::task::JoinHandle`] - Handle to await the final [`PipelineOutput`]
///
/// # Examples
///
/// Basic usage with text prompt:
///
/// ```no_run
/// use asset_tap_core::{PipelineConfig, pipeline::run_pipeline, providers::ProviderRegistry};
///
/// # async fn example() -> anyhow::Result<()> {
/// // Create configuration
/// let config = PipelineConfig::builder()
///     .with_prompt("a cowboy ninja with a leather duster, bandana mask, and dual katanas on the back")
///     ;
///
/// // Get provider registry
/// let registry = ProviderRegistry::new();
///
/// // Run pipeline
/// let (mut progress_rx, handle, _approval_tx, _cancel_tx) = run_pipeline(config, &registry).await?;
///
/// // Monitor progress
/// tokio::spawn(async move {
///     while let Some(progress) = progress_rx.recv().await {
///         println!("Progress: {:?}", progress);
///     }
/// });
///
/// // Wait for completion
/// let output = handle.await??;
/// if let Some(model_path) = output.model_path {
///     println!("Generated model: {}", model_path.display());
/// }
/// # Ok(())
/// # }
/// ```
///
/// Using a template:
///
/// ```no_run
/// # use asset_tap_core::{PipelineConfig, pipeline::run_pipeline, providers::ProviderRegistry};
/// # async fn example() -> anyhow::Result<()> {
/// let config = PipelineConfig::builder()
///     .with_template("character")
///     .with_prompt("a robot warrior")
///     ;
///
/// let registry = ProviderRegistry::new();
/// let (_, handle, _, _) = run_pipeline(config, &registry).await?;
/// let output = handle.await??;
/// # Ok(())
/// # }
/// ```
///
/// Skipping image generation with existing image:
///
/// ```no_run
/// # use asset_tap_core::{PipelineConfig, pipeline::run_pipeline, providers::ProviderRegistry};
/// # async fn example() -> anyhow::Result<()> {
/// let config = PipelineConfig::builder()
///     .with_existing_image("https://example.com/image.png")
///     ;
///
/// let registry = ProviderRegistry::new();
/// let (_, handle, _, _) = run_pipeline(config, &registry).await?;
/// let output = handle.await??;
/// # Ok(())
/// # }
/// ```
pub async fn run_pipeline(
    mut config: PipelineConfig,
    registry: &ProviderRegistry,
) -> Result<(
    tokio::sync::mpsc::UnboundedReceiver<Progress>,
    tokio::task::JoinHandle<Result<PipelineOutput>>,
    Option<tokio::sync::mpsc::UnboundedSender<ApprovalResponse>>,
    tokio::sync::mpsc::UnboundedSender<()>,
)> {
    let (progress_tx, progress_rx) = tokio::sync::mpsc::unbounded_channel();

    // Create approval channel if image approval is required
    let (approval_tx, approval_rx) = tokio::sync::mpsc::unbounded_channel();
    let approval_tx_for_caller = if config.require_image_approval {
        Some(approval_tx.clone())
    } else {
        None
    };
    config.approval_tx = Some(approval_tx);

    // Create cancel channel and shared cancel flag
    let (cancel_tx, cancel_rx) = tokio::sync::mpsc::unbounded_channel();
    let cancel_flag = Arc::new(AtomicBool::new(false));

    // Clone config for the async task
    let config = config.clone();

    let image_provider = resolve_provider(
        registry,
        config.image_provider.as_deref(),
        config.image_model.as_deref(),
        ProviderCapability::TextToImage,
        "image",
    )?;

    let model_3d = config.model_3d.as_str();
    let model_3d_provider = resolve_provider(
        registry,
        config.model_3d_provider.as_deref(),
        (!model_3d.is_empty()).then_some(model_3d),
        ProviderCapability::ImageTo3D,
        "3D",
    )?;

    // Set cancel flag on providers so polling loops can check it
    set_provider_cancel_flag(&image_provider, cancel_flag.clone());
    set_provider_cancel_flag(&model_3d_provider, cancel_flag.clone());

    let handle = tokio::spawn(async move {
        run_pipeline_internal(
            config,
            image_provider,
            model_3d_provider,
            progress_tx,
            approval_rx,
            cancel_rx,
            cancel_flag,
        )
        .await
    });

    Ok((progress_rx, handle, approval_tx_for_caller, cancel_tx))
}

/// Resolve which provider handles a given pipeline stage.
///
/// Resolution order:
///   1. Explicit provider override (e.g. `config.image_provider`).
///   2. Provider that actually exposes `model_id` for the given capability.
///      Prevents routing a `fal-ai/...` model to Meshy (or vice versa) when
///      multiple providers are registered.
///   3. First available provider supporting anything (legacy default).
///
/// `kind` is a human-readable label ("image", "3D") used in error messages.
fn resolve_provider(
    registry: &ProviderRegistry,
    explicit_provider: Option<&str>,
    model_id: Option<&str>,
    capability: ProviderCapability,
    kind: &str,
) -> Result<Arc<dyn Provider>> {
    if let Some(provider_id) = explicit_provider {
        return registry.get(provider_id).ok_or_else(|| {
            Error::InvalidModel(format!("{} provider '{}' not found", kind, provider_id))
        });
    }
    if let Some(id) = model_id {
        return registry
            .find_provider_for_model(capability, id)
            .ok_or_else(|| {
                Error::InvalidModel(format!(
                    "No available provider exposes {} model '{}'",
                    kind, id
                ))
            });
    }
    registry.get_default().ok_or_else(|| {
        Error::MissingApiKey(format!("No providers available for {} generation", kind))
    })
}

/// Set the cancel flag on a provider if it's a DynamicProvider.
fn set_provider_cancel_flag(provider: &Arc<dyn Provider>, flag: Arc<AtomicBool>) {
    if let Some(dp) = provider.as_any().downcast_ref::<DynamicProvider>() {
        dp.set_cancel_flag(flag);
    }
}

/// Log an API error for a pipeline stage and save it to disk.
///
/// Both image generation and 3D generation share the same error-logging
/// pattern.  This helper deduplicates that code.
fn log_stage_error(
    error: &Error,
    stage: Stage,
    gen_dir: &std::path::Path,
    prompt: Option<&str>,
    image_model: Option<&str>,
    model_3d: Option<&str>,
    export_fbx: bool,
) {
    let gen_id = gen_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let error_log = match error {
        Error::ApiError(api_err) => ErrorLog::from_api_error(api_err, Some(stage)),
        _ => ErrorLog::new(ErrorType::ApiError, format!("{}", error))
            .with_stage(stage)
            .with_details(format!("{:?}", error)),
    };
    let error_log = error_log
        .with_generation(gen_id)
        .with_config(ConfigSnapshot {
            prompt: prompt.map(|s| s.to_string()),
            image_model: image_model.map(|s| s.to_string()),
            model_3d: model_3d.map(|s| s.to_string()),
            export_fbx,
            style_ref_count: 0,
        });
    if let Err(save_err) = error_log.save() {
        tracing::warn!("Failed to save error log: {}", save_err);
    }
}

/// Stage 1: Obtain an image — either from an existing file/URL or by generating one.
///
/// Returns the raw image bytes and the resolved image model ID (if generation occurred).
/// Updates `output.image_path` and `output.image_url` as side effects.
#[allow(clippy::too_many_arguments)]
async fn generate_image_stage(
    config: &PipelineConfig,
    image_provider: &Arc<dyn Provider>,
    gen_dir: &std::path::Path,
    output: &mut PipelineOutput,
    progress_tx: &tokio::sync::mpsc::UnboundedSender<Progress>,
    approval_rx: &mut tokio::sync::mpsc::UnboundedReceiver<ApprovalResponse>,
    cancel_flag: &Arc<AtomicBool>,
    image_params: Option<&HashMap<String, serde_json::Value>>,
) -> Result<(Vec<u8>, Option<String>)> {
    let mut resolved_image_model = config.image_model.clone();

    if let Some(ref url) = config.image_url {
        // Use existing image
        let path = PathBuf::from(url);
        if path.exists() {
            // Local file — read bytes and copy to gen_dir
            let dest_path = gen_dir.join(bundle_files::IMAGE);
            std::fs::copy(&path, &dest_path)?;
            output.image_path = Some(dest_path);
            let bytes = std::fs::read(&path)?;
            return Ok((bytes, resolved_image_model));
        } else {
            // Remote URL — download it
            let image_path = gen_dir.join(bundle_files::IMAGE);
            let _ = progress_tx.send(Progress::started(Stage::Download));
            let bytes = download_file(url, &image_path).await?;
            output.image_path = Some(image_path);
            output.image_url = Some(url.clone());
            let _ = progress_tx.send(Progress::completed(Stage::Download));
            return Ok((bytes, resolved_image_model));
        }
    }

    let prompt = config.prompt.as_deref().ok_or_else(|| {
        Error::Validation("Either prompt or image_url must be provided".to_string())
    })?;

    let image_path = gen_dir.join(bundle_files::IMAGE);

    // Resolve which model to use
    let model_id = if let Some(ref model) = config.image_model {
        model.clone()
    } else {
        image_provider
            .get_default_model(ProviderCapability::TextToImage)?
            .id
    };
    resolved_image_model = Some(model_id.clone());

    // Check for cancellation before image generation
    if cancel_flag.load(Ordering::Acquire) {
        return Err(Error::Pipeline("Generation cancelled by user".to_string()));
    }

    let _ = progress_tx.send(Progress::started(Stage::ImageGeneration));

    let mut result = image_provider
        .text_to_image(prompt, &model_id, image_params, Some(progress_tx.clone()))
        .await
        .inspect_err(|e| {
            log_stage_error(
                e,
                Stage::ImageGeneration,
                gen_dir,
                Some(prompt),
                Some(&model_id),
                Some(&config.model_3d),
                config.export_fbx,
            );
        })?;

    // Save image bytes to disk
    std::fs::write(&image_path, &result.data)?;
    output.image_path = Some(image_path.clone());

    // Approval loop: if approval is required, wait for user response.
    // On Regenerate, re-run image generation and loop back.
    // Completed is deferred until after approval so progress messages stay in order.
    if config.require_image_approval {
        loop {
            let _ = progress_tx.send(Progress::completed(Stage::ImageGeneration));

            let approval_data = crate::types::ApprovalData {
                image_path: image_path.clone(),
                image_url: format!("file://{}", image_path.display()),
                prompt: prompt.to_string(),
                model: model_id.clone(),
            };

            let _ = progress_tx.send(Progress::awaiting_approval(
                Stage::ImageGeneration,
                approval_data,
            ));

            tracing::info!("Waiting for user approval...");
            if cancel_flag.load(Ordering::Acquire) {
                return Err(Error::Pipeline("Generation cancelled by user".to_string()));
            }
            match approval_rx.recv().await {
                Some(ApprovalResponse::Approve) => {
                    tracing::info!("Image approved by user, continuing to 3D generation");
                    let _ = progress_tx.send(Progress::processing(
                        Stage::ImageGeneration,
                        Some("Image approved".to_string()),
                    ));
                    break;
                }
                Some(ApprovalResponse::Reject) => {
                    tracing::info!("Image rejected by user, cancelling pipeline");
                    return Err(Error::Pipeline(
                        "Image generation cancelled by user".to_string(),
                    ));
                }
                Some(ApprovalResponse::Regenerate) => {
                    tracing::info!("User requested regeneration, re-running image generation");
                    let _ = progress_tx.send(Progress::started(Stage::ImageGeneration));

                    result = image_provider
                        .text_to_image(prompt, &model_id, image_params, Some(progress_tx.clone()))
                        .await?;

                    // Overwrite image on disk
                    std::fs::write(&image_path, &result.data)?;
                    output.image_path = Some(image_path.clone());
                    // Loop back to send Completed then new AwaitingApproval
                }
                None => {
                    tracing::error!("Approval channel closed unexpectedly!");
                    return Err(Error::Pipeline(
                        "Approval channel closed unexpectedly".to_string(),
                    ));
                }
            }
        }
    } else {
        let _ = progress_tx.send(Progress::completed(Stage::ImageGeneration));
    }

    Ok((result.data, resolved_image_model))
}

/// Stage 2: Generate a 3D model (GLB) from the image bytes.
///
/// Returns the path to the saved GLB file and the resolved 3D model ID.
async fn generate_3d_stage(
    config: &PipelineConfig,
    model_3d_provider: &Arc<dyn Provider>,
    image_data: &[u8],
    gen_dir: &std::path::Path,
    prompt: Option<&str>,
    progress_tx: &tokio::sync::mpsc::UnboundedSender<Progress>,
    model_3d_params: Option<&HashMap<String, serde_json::Value>>,
) -> Result<(PathBuf, String)> {
    let model_3d_id = if !config.model_3d.is_empty() {
        config.model_3d.clone()
    } else {
        model_3d_provider
            .get_default_model(ProviderCapability::ImageTo3D)?
            .id
    };

    let _ = progress_tx.send(Progress::started(Stage::Model3DGeneration));

    let model_result = model_3d_provider
        .image_to_3d(
            image_data,
            &model_3d_id,
            model_3d_params,
            Some(progress_tx.clone()),
        )
        .await
        .inspect_err(|e| {
            log_stage_error(
                e,
                Stage::Model3DGeneration,
                gen_dir,
                prompt,
                config.image_model.as_deref(),
                Some(&model_3d_id),
                config.export_fbx,
            );
        })?;

    let model_path = gen_dir.join(bundle_files::MODEL_GLB);
    std::fs::write(&model_path, &model_result.data)?;
    let _ = progress_tx.send(Progress::completed(Stage::Model3DGeneration));

    Ok((model_path, model_3d_id))
}

/// Stage 3: Optionally convert GLB to FBX via Blender.
///
/// This stage is best-effort — failures are reported via progress but do not
/// fail the pipeline.
fn export_fbx_stage(
    model_path: &std::path::Path,
    output: &mut PipelineOutput,
    progress_tx: &tokio::sync::mpsc::UnboundedSender<Progress>,
    blender_path: Option<&str>,
) {
    let _ = progress_tx.send(Progress::started(Stage::FbxConversion));

    match convert_glb_to_fbx(model_path, blender_path) {
        Ok(Some((fbx_path, textures_dir))) => {
            output.fbx_path = Some(fbx_path);
            output.textures_dir = textures_dir;
            let _ = progress_tx.send(Progress::completed(Stage::FbxConversion));
        }
        Ok(None) => {
            let _ = progress_tx.send(Progress::failed(
                Stage::FbxConversion,
                "Blender not found".to_string(),
            ));
        }
        Err(e) => {
            let _ = progress_tx.send(Progress::failed(Stage::FbxConversion, e.to_string()));
            // Don't fail the whole pipeline for FBX conversion failure
        }
    }
}

/// Internal pipeline implementation.
///
/// Orchestrates the three pipeline stages in sequence:
/// 1. Image acquisition (download, local file, or AI generation)
/// 2. 3D model generation from the image
/// 3. Optional FBX export via Blender
async fn run_pipeline_internal(
    config: PipelineConfig,
    image_provider: Arc<dyn Provider>,
    model_3d_provider: Arc<dyn Provider>,
    progress_tx: tokio::sync::mpsc::UnboundedSender<Progress>,
    mut approval_rx: tokio::sync::mpsc::UnboundedReceiver<ApprovalResponse>,
    cancel_rx: tokio::sync::mpsc::UnboundedReceiver<()>,
    cancel_flag: Arc<AtomicBool>,
) -> Result<PipelineOutput> {
    // Bridge the cancel channel to the atomic flag so polling loops see it immediately.
    let flag_for_bridge = cancel_flag.clone();
    let mut cancel_rx = cancel_rx;
    tokio::spawn(async move {
        if cancel_rx.recv().await.is_some() {
            flag_for_bridge.store(true, Ordering::Release);
            tracing::info!("Cancel flag set via channel bridge");
        }
    });

    let mut output = PipelineOutput::new();
    output.prompt = config.prompt.clone();

    // Validate prompt length before doing any work
    if let Some(ref prompt) = config.prompt {
        use crate::constants::validation;
        if prompt.len() > validation::MAX_PROMPT_LENGTH {
            return Err(Error::Validation(format!(
                "Prompt is too long ({} characters, maximum {})",
                prompt.len(),
                validation::MAX_PROMPT_LENGTH
            )));
        }
    }

    // Create a new generation directory for all output assets
    let gen_dir = if let Some(ref base_dir) = config.output_dir {
        create_generation_dir_in(base_dir)?
    } else {
        create_generation_dir()?
    };
    output.output_dir = Some(gen_dir.clone());

    // Check provider availability
    if !image_provider.is_available() {
        return Err(Error::MissingApiKey(format!(
            "Image provider '{}' is not available (missing API key)",
            image_provider.id()
        )));
    }
    if !model_3d_provider.is_available() {
        return Err(Error::MissingApiKey(format!(
            "3D provider '{}' is not available (missing API key)",
            model_3d_provider.id()
        )));
    }

    // Stage 1: Get or generate image
    let image_params = if config.image_model_params.is_empty() {
        None
    } else {
        Some(&config.image_model_params)
    };
    let (image_data, resolved_image_model) = generate_image_stage(
        &config,
        &image_provider,
        &gen_dir,
        &mut output,
        &progress_tx,
        &mut approval_rx,
        &cancel_flag,
        image_params,
    )
    .await?;

    // Check for cancellation before 3D generation
    if cancel_flag.load(Ordering::Acquire) {
        return Err(Error::Pipeline("Generation cancelled by user".to_string()));
    }

    // Stage 2: Generate 3D model
    let model_3d_params = if config.model_3d_params.is_empty() {
        None
    } else {
        Some(&config.model_3d_params)
    };
    let (model_path, resolved_3d_model) = generate_3d_stage(
        &config,
        &model_3d_provider,
        &image_data,
        &gen_dir,
        output.prompt.as_deref(),
        &progress_tx,
        model_3d_params,
    )
    .await?;
    output.model_path = Some(model_path.clone());

    // Check for cancellation before FBX conversion
    if cancel_flag.load(Ordering::Acquire) {
        return Err(Error::Pipeline("Generation cancelled by user".to_string()));
    }

    // Stage 3: Convert to FBX (optional, best-effort)
    if config.export_fbx {
        export_fbx_stage(
            &model_path,
            &mut output,
            &progress_tx,
            config.blender_path.as_deref(),
        );
    }

    // Extract model stats from the GLB before saving metadata
    let model_info = crate::bundle::extract_model_info(&model_path);

    // Save bundle metadata
    let mut gen_config = GenerationConfig::from(&config);
    gen_config.image_model = resolved_image_model.clone();
    gen_config.model_3d = resolved_3d_model.clone();

    // Record the effective parameter set (YAML defaults merged with user overrides)
    // so bundles are reproducible even if defaults change later. Only records
    // when the model actually ran — a skipped image stage leaves the map empty.
    if let Some(ref model_id) = resolved_image_model {
        gen_config.image_model_params = effective_params(
            image_provider.as_ref(),
            ProviderCapability::TextToImage,
            model_id,
            &config.image_model_params,
        );
    }
    gen_config.model_3d_params = effective_params(
        model_3d_provider.as_ref(),
        ProviderCapability::ImageTo3D,
        &resolved_3d_model,
        &config.model_3d_params,
    );

    let metadata = BundleMetadata {
        version: 1,
        name: None,
        created_at: Utc::now(),
        config: Some(gen_config),
        model_info,
        duration_ms: None,
        tags: Vec::new(),
        favorite: false,
        notes: None,
        generator: Some(crate::bundle::generator_string().to_string()),
        demo_version: None,
    };

    if let Err(e) = metadata.save(&gen_dir) {
        tracing::warn!("Failed to save bundle metadata: {}", e);
    }

    Ok(output)
}

/// Build the effective parameter map for a model by layering user overrides
/// over the YAML-declared defaults.
///
/// Returns the full parameter set actually sent to the provider, so bundles can
/// be reproduced later even if provider defaults change. Returns an empty map
/// if the model declares no parameters, or if the model id can't be resolved
/// against the registry (shouldn't happen after a successful stage, since the
/// stage itself would have failed first).
fn effective_params(
    provider: &dyn Provider,
    capability: ProviderCapability,
    model_id: &str,
    overrides: &HashMap<String, serde_json::Value>,
) -> HashMap<String, serde_json::Value> {
    provider
        .list_models(capability)
        .into_iter()
        .find(|m| m.id == model_id)
        .map(|model| merge_param_overrides(&model.parameters, overrides))
        .unwrap_or_default()
}

/// Merge user-provided overrides on top of a model's declared parameter defaults.
fn merge_param_overrides(
    param_defs: &[crate::providers::config::ParameterDef],
    overrides: &HashMap<String, serde_json::Value>,
) -> HashMap<String, serde_json::Value> {
    let mut effective = HashMap::with_capacity(param_defs.len());
    for param in param_defs {
        let value = overrides
            .get(&param.name)
            .cloned()
            .unwrap_or_else(|| param.default.clone());
        effective.insert(param.name.clone(), value);
    }
    effective
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_config_builder() {
        let config = PipelineConfig::new()
            .with_prompt("a robot")
            .with_3d_model("trellis-2")
            .without_fbx();

        assert_eq!(config.prompt, Some("a robot".to_string()));
        assert_eq!(config.model_3d, "trellis-2");
        assert!(!config.export_fbx);
    }

    #[test]
    fn test_merge_param_overrides_records_defaults() {
        use crate::providers::config::{ParameterDef, ParameterType};

        let param_defs = vec![
            ParameterDef {
                name: "guidance_scale".into(),
                label: "Guidance Scale".into(),
                description: None,
                param_type: ParameterType::Float,
                default: serde_json::json!(3.5),
                min: Some(1.0),
                max: Some(20.0),
                step: Some(0.5),
                options: None,
            },
            ParameterDef {
                name: "topology".into(),
                label: "Topology".into(),
                description: None,
                param_type: ParameterType::Select,
                default: serde_json::json!("triangle"),
                min: None,
                max: None,
                step: None,
                options: Some(vec![
                    serde_json::json!("triangle"),
                    serde_json::json!("quad"),
                ]),
            },
        ];

        // No overrides: YAML defaults are recorded so bundles stay reproducible.
        let effective = merge_param_overrides(&param_defs, &HashMap::new());
        assert_eq!(
            effective.get("guidance_scale"),
            Some(&serde_json::json!(3.5))
        );
        assert_eq!(
            effective.get("topology"),
            Some(&serde_json::json!("triangle"))
        );

        // User override on one param: default kept for the other.
        let mut overrides = HashMap::new();
        overrides.insert("guidance_scale".into(), serde_json::json!(7.0));
        let effective = merge_param_overrides(&param_defs, &overrides);
        assert_eq!(
            effective.get("guidance_scale"),
            Some(&serde_json::json!(7.0))
        );
        assert_eq!(
            effective.get("topology"),
            Some(&serde_json::json!("triangle"))
        );

        // Undeclared override keys are dropped (only declared params are recorded).
        let mut overrides = HashMap::new();
        overrides.insert("not_a_real_param".into(), serde_json::json!("ignored"));
        let effective = merge_param_overrides(&param_defs, &overrides);
        assert!(!effective.contains_key("not_a_real_param"));
        assert_eq!(effective.len(), 2);
    }

    #[test]
    fn test_merge_param_overrides_empty_defs() {
        // Models with no declared params produce an empty map, which
        // serde skips via `skip_serializing_if = "HashMap::is_empty"`.
        let effective = merge_param_overrides(&[], &HashMap::new());
        assert!(effective.is_empty());
    }

    #[test]
    fn test_effective_image_model() {
        // Default - None when not explicitly set (provider will use its default)
        let config = PipelineConfig::new().with_prompt("test");
        assert_eq!(config.effective_image_model(), None);

        // With explicit model override
        let config = PipelineConfig::new()
            .with_prompt("test")
            .with_image_model("flux-1.1-pro");
        assert_eq!(config.effective_image_model(), Some("flux-1.1-pro"));

        // With existing image
        let config = PipelineConfig::new().with_existing_image("http://example.com/image.png");
        assert_eq!(config.effective_image_model(), None);
    }
}
