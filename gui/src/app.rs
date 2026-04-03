//! Main application state and logic.

/// Open a path or URL with the system default handler.
///
/// Logs failures via tracing. If `toasts` is provided, also shows an error toast to the user.
pub fn open_with_system(
    target: impl AsRef<std::ffi::OsStr> + std::fmt::Debug,
    toasts: Option<&mut Vec<Toast>>,
) {
    if let Err(e) = open::that(target.as_ref()) {
        tracing::error!("Failed to open {:?}: {}", target, e);
        if let Some(toasts) = toasts {
            toasts.push(Toast::error(format!("Failed to open: {e}")));
        }
    }
}

/// Embedded logo image for in-app branding (512x512 with "ASSET TAP" text).
const LOGO_BYTES: &[u8] = include_bytes!("../../assets/logo.png");

use crate::constants::{asset_type, callback};
use crate::icons;
use crate::texture_cache::TextureCache;
use crate::viewer::model::{ModelViewer, SharedModelViewer};
use crate::views;
use crate::views::about::AboutModal;
use crate::views::library::LibraryBrowser;
use crate::views::settings::SettingsModal;
use crate::views::walkthrough::Walkthrough;
use crate::views::welcome_modal::WelcomeModal;
use asset_tap_core::constants::files::{DEMO_BUNDLE_SIZE_LABEL, bundle as bundle_files};
use asset_tap_core::{
    bundle::load_bundle,
    history::{ErrorInfo, GenerationHistory},
    pipeline::{PipelineConfig, run_pipeline},
    providers::ProviderCapability,
    settings::{Settings, is_dev_mode},
    state::AppState,
    templates::list_templates,
    types::{ApprovalResponse, PipelineOutput, Progress, Stage},
};
use eframe::egui;
use eframe::glow;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::runtime::Runtime;

/// Result type for async FBX conversion (FBX path, optional textures dir).
type FbxConversionResult = Result<(PathBuf, Option<PathBuf>), String>;

/// A toast notification message shown briefly to the user.
#[derive(Debug, Clone)]
pub struct Toast {
    /// The message to display.
    pub message: String,
    /// Toast type affects styling.
    pub toast_type: ToastType,
    /// When the toast was created.
    pub created_at: Instant,
    /// How long to show the toast (seconds).
    pub duration: f32,
}

/// Type of toast notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastType {
    /// Informational message.
    Info,
    /// Success message.
    Success,
    /// Error message.
    Error,
}

/// Template editor mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TemplateEditorMode {
    /// Viewing a template (read-only for builtins).
    ViewOnly,
    /// Creating a new template.
    #[default]
    Create,
}

impl Toast {
    /// Create a new info toast.
    pub fn info(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            toast_type: ToastType::Info,
            created_at: Instant::now(),
            duration: 3.0,
        }
    }

    /// Create a new success toast.
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            toast_type: ToastType::Success,
            created_at: Instant::now(),
            duration: 3.0,
        }
    }

    /// Create a new error toast (stays visible longer).
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            toast_type: ToastType::Error,
            created_at: Instant::now(),
            duration: 5.0,
        }
    }

    /// Check if the toast should still be shown.
    pub fn is_visible(&self) -> bool {
        self.created_at.elapsed().as_secs_f32() < self.duration
    }

    /// Get the opacity (fades out near end).
    pub fn opacity(&self) -> f32 {
        use crate::constants::timing;

        let elapsed = self.created_at.elapsed().as_secs_f32();
        let remaining = self.duration - elapsed;
        if remaining < timing::TOAST_FADE_OUT_DURATION {
            (remaining / timing::TOAST_FADE_OUT_DURATION).max(0.0)
        } else if elapsed < timing::TOAST_FADE_IN_DURATION {
            elapsed / timing::TOAST_FADE_IN_DURATION
        } else {
            1.0
        }
    }
}

/// Main application state.
pub struct App {
    /// Tokio runtime for async operations.
    runtime: Runtime,

    // =========================================================================
    // Pipeline Configuration (bound to UI)
    // =========================================================================
    /// Text prompt input.
    pub prompt: String,

    /// Selected prompt template.
    pub template: Option<String>,

    /// Cached effective prompt length: (prompt, template, result).
    cached_effective_prompt_len: (String, Option<String>, usize),

    /// Selected provider for image generation.
    pub image_provider: String,

    /// Selected image model.
    pub image_model: String,

    /// Selected provider for 3D generation.
    pub model_3d_provider: String,

    /// Selected 3D model.
    pub model_3d: String,

    /// Current parameter overrides for the selected image model.
    pub image_model_params: std::collections::HashMap<String, serde_json::Value>,

    /// Current parameter overrides for the selected 3D model.
    pub model_3d_params: std::collections::HashMap<String, serde_json::Value>,

    /// Whether to export FBX.
    pub export_fbx: bool,

    /// Existing image path/URL (skips image generation).
    pub existing_image: Option<String>,

    // =========================================================================
    // Pipeline State
    // =========================================================================
    /// Shared pipeline state (for communication with async task).
    pub state: Arc<Mutex<PipelineState>>,

    /// Most recent pipeline output.
    pub output: Option<PipelineOutput>,

    // =========================================================================
    // UI State
    // =========================================================================
    /// Currently selected preview tab.
    pub preview_tab: PreviewTab,

    /// Available templates (cached).
    pub available_templates: Vec<String>,

    /// Provider registry (cached to avoid recreating on every frame).
    pub provider_registry: asset_tap_core::providers::ProviderRegistry,

    /// 3D model viewer (shared for PaintCallback).
    pub model_viewer: SharedModelViewer,

    /// Library browser for selecting past generations.
    pub library_browser: LibraryBrowser,

    /// Texture thumbnail cache with background loading.
    pub texture_cache: TextureCache,

    /// Full-resolution texture for image approval modal (loaded on demand).
    pub approval_texture: Option<(PathBuf, egui::TextureHandle)>,

    /// Glow context for 3D rendering (exposed for model viewer).
    pub gl_context: Option<Arc<glow::Context>>,

    /// Bundle info panel for displaying and editing current bundle metadata.
    pub bundle_info_panel: views::bundle_info::BundleInfoPanel,

    /// Confirmation dialog for loading associated assets.
    pub confirmation_dialog: views::confirmation_dialog::ConfirmationDialog,

    // =========================================================================
    // Settings
    // =========================================================================
    /// User settings (persisted to disk).
    pub settings: Settings,

    /// Welcome modal for first-time setup.
    pub welcome_modal: WelcomeModal,

    /// Settings modal for editing configuration.
    pub settings_modal: SettingsModal,

    /// About modal.
    pub about_modal: AboutModal,

    /// App logo texture (loaded once at startup).
    pub logo_texture: Option<egui::TextureHandle>,

    /// Cached Blender availability (checked at startup and on settings save).
    pub blender_available: bool,

    // =========================================================================
    // State & History
    // =========================================================================
    /// Application state (for session recovery).
    pub app_state: AppState,

    /// Generation history (for tracking all runs).
    pub history: Arc<Mutex<GenerationHistory>>,

    /// Current generation ID (set when pipeline starts).
    pub current_generation_id: Option<String>,

    // =========================================================================
    // Pending Bundle Load
    // =========================================================================
    /// Pending bundle load (waiting for confirmation dialog).
    /// Stores (output, parent_dir, asset_type) until user confirms.
    pub pending_bundle_load: Option<(PipelineOutput, PathBuf, String)>,

    // =========================================================================
    // Pending File Dialog
    // =========================================================================
    /// Pending file selection result (from async file dialog).
    pub pending_file_selection: Option<tokio::sync::oneshot::Receiver<Option<PathBuf>>>,

    /// Pending export result (from async zip creation).
    pending_export: Option<tokio::sync::oneshot::Receiver<Result<String, String>>>,

    /// Pending FBX conversion result (from async Blender conversion).
    pub pending_fbx_conversion: Option<tokio::sync::oneshot::Receiver<FbxConversionResult>>,

    /// Pending demo bundle download result (from async download).
    pending_demo_download:
        Option<tokio::sync::oneshot::Receiver<Result<asset_tap_core::DemoDownloadResult, String>>>,

    /// Pending bundle import result (from async zip extraction).
    pending_import: Option<tokio::sync::oneshot::Receiver<Result<std::path::PathBuf, String>>>,

    /// Whether to show the demo download confirmation dialog.
    show_demo_download_confirm: bool,

    /// Bundle path pending deletion (waiting for confirmation).
    pending_delete_bundle: Option<std::path::PathBuf>,

    // =========================================================================
    // Toast Notifications
    // =========================================================================
    /// Active toast notifications.
    pub toasts: Vec<Toast>,

    /// Whether the error toast has been shown for the current pipeline run.
    error_toast_shown: bool,

    // =========================================================================
    // Template Editor
    // =========================================================================
    /// Whether the template editor modal is open.
    pub show_template_editor: bool,

    /// Template currently being edited (None = creating new).
    pub editing_template: Option<asset_tap_core::templates::TemplateDefinition>,

    /// Template editor: name input.
    pub editor_name_input: String,

    /// Template editor: description input.
    pub editor_description_input: String,

    /// Template editor: template syntax input.
    pub editor_template_input: String,

    /// Template editor: error message.
    pub editor_error: Option<String>,

    /// Template editor mode.
    pub editor_mode: TemplateEditorMode,

    // =========================================================================
    // Confirmation Dialogs
    // =========================================================================
    /// Whether to show the clear history confirmation dialog.
    pub show_clear_history_confirmation: bool,

    // =========================================================================
    // Walkthrough
    // =========================================================================
    /// Interactive walkthrough for new users.
    pub walkthrough: Walkthrough,
}

/// Pipeline execution state.
#[derive(Default)]
pub struct PipelineState {
    /// Whether the pipeline is currently running.
    pub running: bool,

    /// Progress messages.
    pub progress: Vec<Progress>,

    /// Current stage.
    pub current_stage: Option<Stage>,

    /// Error message (if failed).
    pub error: Option<String>,

    /// Completed output (set when pipeline finishes).
    pub completed_output: Option<PipelineOutput>,

    /// Recovery info for failed generations.
    /// Contains path to recoverable image if image generation succeeded
    /// but a later stage failed.
    pub recovery_info: Option<RecoveryInfo>,

    /// Awaiting user approval for generated image.
    /// When set, the pipeline is paused and waiting for user input.
    pub awaiting_approval: Option<asset_tap_core::types::ApprovalData>,

    /// Channel for sending approval responses back to the pipeline.
    pub approval_tx: Option<tokio::sync::mpsc::UnboundedSender<ApprovalResponse>>,

    /// Channel for cancelling the running pipeline.
    pub cancel_tx: Option<tokio::sync::mpsc::UnboundedSender<()>>,

    /// Whether the pipeline is currently regenerating the image (user clicked Regenerate).
    pub regenerating_image: bool,

    /// When true, the next pipeline run should keep existing progress logs
    /// instead of clearing them. Set by recovery flow so the full event
    /// history is preserved when the user proceeds with a saved image.
    pub preserve_progress: bool,
}

/// Information for recovering from a failed generation.
#[derive(Debug, Clone)]
pub struct RecoveryInfo {
    /// Path to the image that was successfully generated.
    pub image_path: PathBuf,
    /// User-friendly description of recovery option.
    pub recovery_message: String,
    /// Button label for the recovery action.
    pub button_label: String,
}

/// Preview tab selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreviewTab {
    #[default]
    Image,
    Model3D,
    Textures,
}

impl App {
    /// Create a new application instance.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Get the glow context if available
        let gl_context = cc.gl.clone();

        // Load settings from disk
        let mut settings = Settings::load();

        // Get default provider and models from registry
        // IMPORTANT: Create registry once and reuse it to avoid performance issues
        let provider_registry = asset_tap_core::providers::ProviderRegistry::new();

        // In dev mode, sync FROM environment TO settings first (so .env keys are picked up)
        if is_dev_mode() {
            settings.sync_from_env(&provider_registry);
            // Ensure dev mode uses .dev/output (in case settings.json has wrong value)
            settings.output_dir = PathBuf::from(".dev/output");
        }

        // Sync settings to environment variables (for GUI app)
        // This ensures providers can access keys via env vars
        settings.sync_to_env(&provider_registry);

        // Load app state early (needed for welcome modal check)
        let app_state = AppState::load();

        // Create welcome modal and open if user wants to see it on startup
        let mut welcome_modal = WelcomeModal::new(settings.output_dir.clone());
        if app_state.show_welcome_on_startup {
            welcome_modal.open();
        }
        let default_provider = provider_registry.get_default();

        let (default_image_provider, default_3d_provider) = if let Some(provider) = default_provider
        {
            (provider.id().to_string(), provider.id().to_string())
        } else {
            (String::new(), String::new())
        };

        // Get default models from provider registry
        let default_image_model = provider_registry
            .get_default()
            .and_then(|provider| {
                let models = provider.list_models(ProviderCapability::TextToImage);
                models
                    .iter()
                    .find(|m| m.is_default)
                    .or_else(|| models.first())
                    .map(|m| m.id.clone())
            })
            .unwrap_or_default();
        let default_3d_model = provider_registry
            .get_default()
            .and_then(|provider| {
                let models = provider.list_models(ProviderCapability::ImageTo3D);
                models
                    .iter()
                    .find(|m| m.is_default)
                    .or_else(|| models.first())
                    .map(|m| m.id.clone())
            })
            .unwrap_or_default();

        // Restore persisted model selections, falling back to defaults
        let image_provider = app_state
            .selected_image_provider
            .clone()
            .unwrap_or(default_image_provider);
        let image_model = app_state
            .selected_image_model
            .clone()
            .unwrap_or(default_image_model);
        let model_3d_provider = app_state
            .selected_3d_provider
            .clone()
            .unwrap_or(default_3d_provider);
        let model_3d = app_state
            .selected_3d_model
            .clone()
            .unwrap_or(default_3d_model);

        // Load and clean up history (mark any in-progress as interrupted)
        let mut history = GenerationHistory::load();
        history.mark_interrupted();

        // Start with empty prompt (fresh slate for new session)
        let prompt = String::new();

        // Restore preview tab
        let preview_tab = match app_state.preview_tab.as_str() {
            "Image" => PreviewTab::Image,
            "Textures" => PreviewTab::Textures,
            _ => PreviewTab::Model3D,
        };

        // Try to restore the current generation being viewed using standardized bundle loading
        let output = app_state.current_generation.as_ref().and_then(|dir| {
            // Use the core bundle loading logic for consistent file discovery
            load_bundle(dir).ok().map(PipelineOutput::from)
        });

        let runtime = Runtime::new().expect("Failed to create Tokio runtime");

        let mut app = Self {
            runtime,

            // Configuration defaults (from settings or registry)
            prompt,
            template: None,
            cached_effective_prompt_len: (String::new(), None, 0),
            image_provider,
            image_model,
            model_3d_provider,
            model_3d,
            image_model_params: std::collections::HashMap::new(),
            model_3d_params: std::collections::HashMap::new(),
            export_fbx: settings.export_fbx_default,
            existing_image: None,

            // State
            state: Arc::new(Mutex::new(PipelineState::default())),
            output,

            // UI
            preview_tab,
            available_templates: list_templates(),
            provider_registry, // Reuse the registry created above
            model_viewer: Arc::new(Mutex::new(ModelViewer::new())),
            library_browser: LibraryBrowser::new(),
            texture_cache: TextureCache::new(),
            approval_texture: None,
            bundle_info_panel: views::bundle_info::BundleInfoPanel::new(),
            confirmation_dialog: views::confirmation_dialog::ConfirmationDialog::new(),

            // Rendering
            gl_context,

            // Settings
            settings,
            welcome_modal,
            settings_modal: SettingsModal::new(),
            about_modal: AboutModal::new(),
            logo_texture: None, // Loaded after context is available
            blender_available: asset_tap_core::convert::find_blender().is_some(),

            // State & History
            app_state,
            history: Arc::new(Mutex::new(history)),
            current_generation_id: None,

            // Pending Bundle Load
            pending_bundle_load: None,

            // Pending File Dialog
            pending_file_selection: None,
            pending_export: None,
            pending_fbx_conversion: None,
            pending_demo_download: None,
            pending_import: None,
            show_demo_download_confirm: false,
            pending_delete_bundle: None,

            // Toast Notifications
            toasts: Vec::new(),
            error_toast_shown: false,

            // Template Editor
            show_template_editor: false,
            editing_template: None,
            editor_name_input: String::new(),
            editor_description_input: String::new(),
            editor_template_input: String::new(),
            editor_error: None,
            editor_mode: TemplateEditorMode::default(),

            // Confirmation Dialogs
            show_clear_history_confirmation: false,

            // Walkthrough
            walkthrough: Walkthrough::new(),
        };

        // Restore model info from state to viewer
        if let Some(ref state_info) = app.app_state.model_info {
            let mut viewer = app.model_viewer.lock().unwrap();
            viewer.model_info = Some(crate::viewer::model::ModelInfo {
                file_size: state_info.file_size,
                format: state_info.format.clone(),
                vertex_count: state_info.vertex_count,
                triangle_count: state_info.triangle_count,
            });
        }

        // Restore bundle info panel from current generation
        if let Some(ref current_gen) = app.app_state.current_generation
            && let Err(e) = app.bundle_info_panel.load_bundle(current_gen.clone())
        {
            tracing::warn!("Failed to load bundle metadata on startup: {}", e);
        }

        // Discovery is disabled — using curated static models from provider YAML.
        // To discover new models for evaluation, use: make refresh-models

        // Only access output_dir eagerly if the welcome modal won't be shown.
        // Otherwise, defer until the welcome modal closes — accessing ~/Documents
        // before the user configures the path triggers a macOS permission prompt.
        if !app.app_state.show_welcome_on_startup {
            // Populate bundle selector dropdown
            app.bundle_info_panel
                .refresh_bundle_list(&app.settings.output_dir);
        }

        // Load app logo texture
        app.load_logo_texture(&cc.egui_ctx);

        app
    }

    /// Start downloading the demo bundle in the background.
    ///
    /// Fetches the manifest to check version, then downloads if needed.
    /// Does nothing if a download is already in progress.
    fn start_demo_download(&mut self) {
        if self.pending_demo_download.is_some() {
            return; // Already downloading
        }

        let output_dir = self.settings.output_dir.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending_demo_download = Some(rx);
        self.toasts.push(Toast::info("Checking for demo bundle..."));
        self.runtime.spawn(async move {
            let result = asset_tap_core::download_demo_bundle(output_dir, |_progress| {}).await;
            let _ = tx.send(result.map_err(|e| e.to_string()));
        });
    }

    /// Import a bundle from a zip file in the background.
    fn import_bundle(&mut self, source: std::path::PathBuf) {
        let output_dir = self.settings.output_dir.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending_import = Some(rx);
        self.add_toast(Toast::info("Importing bundle..."));
        self.runtime.spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                asset_tap_core::import_bundle_zip(&source, &output_dir)
            })
            .await
            .unwrap_or_else(|e| Err(format!("Import task failed: {}", e)));
            let _ = tx.send(result);
        });
    }

    /// Load the app logo texture from embedded bytes.
    fn load_logo_texture(&mut self, ctx: &egui::Context) {
        match image::load_from_memory(LOGO_BYTES) {
            Ok(img) => {
                let rgba = img.to_rgba8();
                let (width, height) = rgba.dimensions();
                let color_image = egui::ColorImage::from_rgba_unmultiplied(
                    [width as usize, height as usize],
                    rgba.as_raw(),
                );
                self.logo_texture =
                    Some(ctx.load_texture("app_logo", color_image, egui::TextureOptions::LINEAR));
            }
            Err(e) => {
                tracing::error!("Failed to load app logo: {}", e);
            }
        }
    }

    /// Add a toast notification.
    pub fn add_toast(&mut self, toast: Toast) {
        self.toasts.push(toast);
    }

    /// Remove expired toasts.
    fn cleanup_toasts(&mut self) {
        self.toasts.retain(|t| t.is_visible());
    }

    /// Returns the effective prompt length, accounting for template expansion.
    /// Cached to avoid repeated registry lookups + interpolation per frame.
    pub fn effective_prompt_len(&mut self) -> usize {
        if self.prompt == self.cached_effective_prompt_len.0
            && self.template == self.cached_effective_prompt_len.1
        {
            return self.cached_effective_prompt_len.2;
        }
        let key = (self.prompt.clone(), self.template.clone());
        let result = if let Some(ref template) = self.template {
            asset_tap_core::templates::apply_template(template, &self.prompt)
                .map_or(self.prompt.len(), |expanded| expanded.len())
        } else {
            self.prompt.len()
        };
        self.cached_effective_prompt_len = (key.0, key.1, result);
        result
    }

    /// Check if the pipeline can be started.
    pub fn can_generate(&mut self) -> bool {
        let running = self.state.lock().unwrap().running;
        let has_image_model = self.existing_image.is_some()
            || (!self.image_provider.is_empty() && !self.image_model.is_empty());
        !running
            && (!self.prompt.is_empty() || self.existing_image.is_some())
            && self.effective_prompt_len()
                <= asset_tap_core::constants::validation::MAX_PROMPT_LENGTH
            && self.settings.has_required_api_keys(&self.provider_registry)
            && has_image_model
            && !self.model_3d_provider.is_empty()
            && !self.model_3d.is_empty()
    }

    /// Get the reason why generation is disabled (if applicable).
    pub fn generate_disabled_reason(&mut self) -> Option<String> {
        let running = self.state.lock().unwrap().running;
        if running {
            return Some("Generation in progress".to_string());
        }
        if self.prompt.is_empty() && self.existing_image.is_none() {
            return Some("Enter a prompt or select an image".to_string());
        }
        let max_len = asset_tap_core::constants::validation::MAX_PROMPT_LENGTH;
        let effective_len = self.effective_prompt_len();
        if effective_len > max_len {
            return Some(format!(
                "Prompt too long ({}/{} characters{})",
                effective_len,
                max_len,
                if self.template.is_some() {
                    " after template expansion"
                } else {
                    ""
                }
            ));
        }
        if !self.settings.has_required_api_keys(&self.provider_registry) {
            return Some(
                "API key required. Configure in Settings to enable generation.".to_string(),
            );
        }
        let has_image_model = self.existing_image.is_some()
            || (!self.image_provider.is_empty() && !self.image_model.is_empty());
        if !has_image_model {
            return Some("Select an image model to generate with.".to_string());
        }
        if self.model_3d_provider.is_empty() || self.model_3d.is_empty() {
            return Some("Select a 3D model to generate with.".to_string());
        }
        None
    }

    /// Cancel the running pipeline.
    pub fn cancel_pipeline(&mut self) {
        let mut state = self.state.lock().unwrap();
        if let Some(cancel_tx) = state.cancel_tx.take() {
            let _ = cancel_tx.send(());
            tracing::info!("Cancel signal sent to pipeline");
        }
        // Mark as cancelled in history
        if let Some(ref gen_id) = self.current_generation_id {
            let mut history = self.history.lock().unwrap();
            history.cancel_generation(gen_id);
        }
    }

    /// Start the pipeline execution.
    pub fn run_pipeline(&mut self) {
        // Build configuration
        let mut config = PipelineConfig::new()
            .with_image_provider(&self.image_provider)
            .with_3d_provider(&self.model_3d_provider)
            .with_3d_model(&self.model_3d)
            .with_output_dir(self.settings.output_dir.clone())
            .with_image_model_params(self.image_model_params.clone())
            .with_3d_model_params(self.model_3d_params.clone());

        let prompt = if let Some(ref image) = self.existing_image {
            // Using a reference image — skip prompt/template since image generation is bypassed
            config = config.with_existing_image(image);
            String::new()
        } else {
            // Apply template if selected
            let prompt = if let Some(ref template) = self.template {
                asset_tap_core::templates::apply_template(template, &self.prompt)
                    .unwrap_or_else(|| self.prompt.clone())
            } else {
                self.prompt.clone()
            };

            if !prompt.is_empty() {
                config = config.with_prompt(prompt.clone());
            }

            // Store original user input when a template was used
            if self.template.is_some() && !self.prompt.is_empty() {
                config = config.with_user_prompt(&self.prompt);
            }

            // Store the template name in config
            if let Some(ref template) = self.template {
                config = config.with_template(template);
            }

            // Always set the image model (since it's provider-specific now)
            config = config.with_image_model(&self.image_model);

            prompt
        };

        let has_custom_blender = self
            .settings
            .blender_path
            .as_ref()
            .is_some_and(|p| !p.is_empty());

        if !self.export_fbx || (!self.blender_available && !has_custom_blender) {
            config = config.without_fbx();
        }

        if let Some(ref blender) = self.settings.blender_path
            && !blender.is_empty()
        {
            config = config.with_blender_path(blender);
        }

        // Enable approval if required by settings (and image is being generated, not using existing)
        if self.settings.require_image_approval && self.existing_image.is_none() {
            config = config.with_image_approval();
        }

        // Start tracking in history
        let generation_id = {
            let mut history = self.history.lock().unwrap();
            history.start_generation(&config, Some(&self.provider_registry))
        };
        self.current_generation_id = Some(generation_id.clone());

        // Update app state for crash recovery
        self.app_state.start_generation(&generation_id);
        self.app_state.last_prompt = if prompt.is_empty() {
            None
        } else {
            Some(prompt.clone())
        };

        // Add to prompt history (if not empty and not duplicate of most recent)
        // Store the raw user input, not the interpolated prompt, so re-selecting
        // a history entry doesn't double-apply the template.
        // Skip when using a reference image — the prompt wasn't used for generation.
        if !self.prompt.is_empty() && self.existing_image.is_none() {
            let entry = asset_tap_core::state::PromptHistoryEntry {
                prompt: self.prompt.clone(),
                template: self.template.clone(),
            };

            // Check if most recent entry is the same (comparing both prompt and template)
            let is_duplicate = self
                .app_state
                .prompt_history
                .first()
                .map(|e| e.prompt == entry.prompt && e.template == entry.template)
                .unwrap_or(false);

            if !is_duplicate {
                self.app_state.prompt_history.insert(0, entry);
                // Keep max 20 prompts
                self.app_state.prompt_history.truncate(20);
            }
        }

        // Reset pipeline state
        {
            let mut state = self.state.lock().unwrap();
            state.running = true;
            if !state.preserve_progress {
                state.progress.clear();
            }
            state.preserve_progress = false;
            state.current_stage = None;
            state.error = None;
            state.completed_output = None;
            state.recovery_info = None;
            state.awaiting_approval = None;
            state.approval_tx = None; // Will be set after pipeline starts if approval is required
            state.cancel_tx = None; // Will be set after pipeline starts
        }
        self.error_toast_shown = false;

        // Clone state for the async task
        let state = self.state.clone();
        let history = self.history.clone();
        let gen_id = generation_id;
        let output_dir = self.settings.output_dir.clone();
        let registry = self.provider_registry.clone();

        // Spawn the pipeline
        self.runtime.spawn(async move {

            // Track completed stages for recovery
            let completed_stages = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

            let result = async {
                // Start the pipeline
                let (mut progress_rx, handle, approval_tx, cancel_tx) = run_pipeline(config, &registry).await?;

                // Store cancel sender so UI can cancel the pipeline
                state.lock().unwrap().cancel_tx = Some(cancel_tx);

                // Store approval sender if image approval is enabled
                if let Some(tx) = approval_tx {
                    tracing::info!("Storing approval channel for image approval");
                    state.lock().unwrap().approval_tx = Some(tx);
                } else {
                    tracing::debug!("No approval channel returned (image approval not required)");
                }

                // Process progress updates
                while let Some(progress) = progress_rx.recv().await {
                    let mut s = state.lock().unwrap();

                    // Update current stage and track completions
                    match &progress {
                        Progress::Started { stage, .. } => s.current_stage = Some(*stage),
                        Progress::Completed { stage, .. } => {
                            if s.current_stage == Some(*stage) {
                                s.current_stage = None;
                            }
                            // Track completed stages for recovery
                            completed_stages.lock().unwrap().push(*stage);
                        }
                        Progress::Failed { stage, .. } => {
                            if s.current_stage == Some(*stage) {
                                s.current_stage = None;
                            }
                        }
                        Progress::AwaitingApproval { approval_data, .. } => {
                            // Store approval data and pause pipeline
                            // Clear regenerating flag (new image is ready for review)
                            s.awaiting_approval = Some(approval_data.clone());
                            s.regenerating_image = false;
                        }
                        _ => {}
                    }

                    // For transient status updates (Queued, Processing), replace the
                    // last event of the same type/stage instead of appending a new line.
                    // This prevents the progress pane from flooding with hundreds of
                    // "Processing... (Ns elapsed)" lines during long polling waits.
                    // Only replace within the current run of transient events — don't
                    // reach back past Completed/AwaitingApproval boundaries.
                    let replace_pos = match &progress {
                        Progress::Queued { stage, .. } | Progress::Processing { stage, .. } => {
                            let target_stage = *stage;
                            // Find the last Completed or AwaitingApproval for this stage
                            // to avoid replacing across stage boundaries.
                            let boundary = s.progress.iter().rposition(|p| matches!(
                                p,
                                Progress::Completed { stage: s, .. }
                                    | Progress::AwaitingApproval { stage: s, .. }
                                    if *s == target_stage
                            ));
                            s.progress.iter().rposition(|p| matches!(
                                p,
                                Progress::Queued { stage: s, .. }
                                    | Progress::Processing { stage: s, .. }
                                    if *s == target_stage
                            )).filter(|pos| boundary.is_none_or(|b| *pos > b))
                        }
                        _ => None,
                    };
                    if let Some(pos) = replace_pos {
                        s.progress[pos] = progress;
                    } else {
                        s.progress.push(progress);
                    }
                }

                // Wait for pipeline to complete
                handle.await.map_err(|e| {
                    asset_tap_core::types::Error::Pipeline(format!("Pipeline task failed: {}", e))
                })?
            }
            .await;

            // Update final state and history
            let mut s = state.lock().unwrap();
            s.running = false;

            match result {
                Ok(output) => {
                    // Record success in history
                    {
                        let mut h = history.lock().unwrap();
                        h.complete_generation(&gen_id, &output);
                    }
                    s.completed_output = Some(output);
                }
                Err(e) => {
                    let error_message = e.to_string();
                    let failed_stage = s.current_stage;

                    // Clear current_stage so the spinner stops
                    s.current_stage = None;

                    // Add a Failed progress entry for the stage that was running.
                    // Use a short message since the full error is shown separately below.
                    if let Some(stage) = failed_stage {
                        s.progress
                            .push(Progress::failed(stage, "see error below".to_string()));
                    }

                    // Check for recovery opportunity:
                    // If image generation or upload completed, we can retry with that image
                    let completed = completed_stages.lock().unwrap();
                    let image_stage_completed =
                        completed.contains(&Stage::ImageGeneration);

                    if image_stage_completed {
                        // Look for the saved image in the generation directory
                        // The generation directory is timestamped, find the most recent one
                        if let Ok(entries) = std::fs::read_dir(&output_dir) {
                            let mut dirs: Vec<_> = entries
                                .filter_map(|e| e.ok())
                                .filter(|e| e.path().is_dir())
                                .collect();
                            // Sort by name descending (newest first based on timestamp)
                            dirs.sort_by_key(|d| std::cmp::Reverse(d.file_name()));

                            if let Some(latest_dir) = dirs.first() {
                                let image_path = latest_dir.path().join(bundle_files::IMAGE);
                                if image_path.exists() {
                                    // Check if this was a user rejection (cancelled by user)
                                    let is_user_rejection = error_message.contains("cancelled by user");
                                    s.recovery_info = Some(RecoveryInfo {
                                        image_path,
                                        recovery_message: if is_user_rejection {
                                            "Image was saved. You can still proceed with 3D generation using this image.".to_string()
                                        } else {
                                            "Image was saved. You can retry 3D generation with this image.".to_string()
                                        },
                                        button_label: if is_user_rejection {
                                            "Proceed with this image".to_string()
                                        } else {
                                            "Retry with saved image".to_string()
                                        },
                                    });
                                }
                            }
                        }
                    }

                    // Record failure in history
                    {
                        let mut h = history.lock().unwrap();
                        h.fail_generation(
                            &gen_id,
                            ErrorInfo {
                                message: error_message.clone(),
                                stage: failed_stage.map(|st| st.to_string()),
                                details: None,
                                log_file: None,
                                partial_output: s.recovery_info.as_ref().map(|r| {
                                    asset_tap_core::history::GenerationOutput {
                                        output_dir: r.image_path.parent().map(|p| p.to_path_buf()),
                                        image_path: Some(r.image_path.clone()),
                                        model_path: None,
                                        fbx_path: None,
                                        textures_dir: None,
                                    }
                                }),
                            },
                        );
                    }
                    s.error = Some(error_message);
                }
            }
        });
    }

    /// Select an existing image to use.
    ///
    /// Uses async file dialog to avoid panics on macOS.
    pub fn select_existing_image(&mut self) {
        // Create a oneshot channel for the result
        let (tx, rx) = tokio::sync::oneshot::channel();

        // Spawn the async file dialog on the runtime
        self.runtime.spawn(async move {
            let result = rfd::AsyncFileDialog::new()
                .add_filter("Images", &["png", "jpg", "jpeg", "webp"])
                .pick_file()
                .await
                .map(|handle| handle.path().to_path_buf());

            let _ = tx.send(result);
        });

        // Store the receiver to check in update()
        self.pending_file_selection = Some(rx);
    }

    /// Set an existing image from a path (used for drag-and-drop).
    pub fn set_existing_image(&mut self, path: String) -> bool {
        // Validate the file extension
        let valid_extensions = ["png", "jpg", "jpeg", "webp"];
        if let Some(ext) = std::path::Path::new(&path)
            .extension()
            .and_then(|e| e.to_str())
            && valid_extensions.contains(&ext.to_lowercase().as_str())
        {
            self.existing_image = Some(path);
            return true;
        }
        false
    }

    /// Clear the existing image selection.
    pub fn clear_existing_image(&mut self) {
        self.existing_image = None;
    }

    /// Open library browser for existing image selection.
    pub fn open_library_for_existing_image(&mut self) {
        self.library_browser
            .open_for_images(callback::EXISTING_IMAGE);
    }

    /// Open library browser for selecting an image to preview.
    pub fn open_library_for_image_preview(&mut self) {
        self.library_browser
            .open_for_images(callback::PREVIEW_IMAGE);
    }

    /// Open library browser for selecting a model to preview.
    pub fn open_library_for_model_preview(&mut self) {
        self.library_browser
            .open_for_models(callback::PREVIEW_MODEL);
    }

    /// Open library browser for selecting textures to preview.
    pub fn open_library_for_textures_preview(&mut self) {
        self.library_browser
            .open_for_textures(callback::PREVIEW_TEXTURES);
    }

    /// Handle approval of generated image - send approval to pipeline.
    pub fn approve_generated_image(&mut self) {
        self.approval_texture = None;
        let mut state = self.state.lock().unwrap();
        state.awaiting_approval = None;

        // Send approval to pipeline
        if let Some(ref tx) = state.approval_tx {
            tracing::info!("Sending approval signal to pipeline");
            match tx.send(ApprovalResponse::Approve) {
                Ok(_) => tracing::info!("Approval signal sent successfully"),
                Err(e) => tracing::error!("Failed to send approval signal: {}", e),
            }
        } else {
            tracing::error!("No approval channel available! This is a bug.");
        }
    }

    /// Handle rejection of generated image - send rejection to pipeline.
    pub fn reject_generated_image(&mut self) {
        self.approval_texture = None;
        {
            let mut state = self.state.lock().unwrap();
            state.awaiting_approval = None;

            // Send rejection to pipeline
            if let Some(ref tx) = state.approval_tx {
                let _ = tx.send(ApprovalResponse::Reject);
            }
        } // Release lock before add_toast

        self.add_toast(Toast::info(
            "Generation cancelled. You can modify your prompt and try again.",
        ));
    }

    /// Handle regeneration request - tell pipeline to regenerate in-place.
    pub fn regenerate_image(&mut self) {
        let mut state = self.state.lock().unwrap();

        // Invalidate the texture cache for the current image so the new one gets loaded
        if let Some(ref approval_data) = state.awaiting_approval {
            self.texture_cache.invalidate(&approval_data.image_path);
        }
        // Clear full-resolution approval texture
        self.approval_texture = None;

        // Clear the approval data (modal will show loading state)
        state.awaiting_approval = None;
        state.regenerating_image = true;

        // Send regenerate signal to pipeline (it will re-run image generation and send new AwaitingApproval)
        if let Some(ref tx) = state.approval_tx {
            let _ = tx.send(ApprovalResponse::Regenerate);
        }
    }

    /// Start an async FBX conversion for the given GLB file.
    ///
    /// Spawns the conversion on the tokio runtime so the GUI remains responsive.
    /// Results are polled via `pending_fbx_conversion` in the update loop.
    pub fn start_fbx_conversion(&mut self, glb_path: PathBuf) {
        let blender_path = self.settings.blender_path.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending_fbx_conversion = Some(rx);
        self.add_toast(Toast::info("Converting to FBX..."));
        self.runtime.spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                asset_tap_core::convert::convert_glb_to_fbx(&glb_path, blender_path.as_deref())
            })
            .await;
            let msg = match result {
                Ok(Ok(Some((fbx, textures)))) => Ok((fbx, textures)),
                Ok(Ok(None)) => {
                    Err("Blender not found. Install Blender to enable FBX conversion.".to_string())
                }
                Ok(Err(e)) => Err(format!("FBX conversion failed: {}", e)),
                Err(e) => Err(format!("FBX conversion task failed: {}", e)),
            };
            let _ = tx.send(msg);
        });
    }

    /// Open an FBX file in Blender.
    ///
    /// Uses the same Blender detection logic as the FBX conversion process.
    /// Shows a toast notification if Blender is not found.
    pub fn open_fbx_in_blender(&mut self, fbx_path: &std::path::Path) {
        use asset_tap_core::convert::find_blender;

        match find_blender() {
            Some(blender_cmd) => {
                // Launch Blender with a Python script to import the FBX
                // (Blender can't open FBX files directly, it needs to import them)
                let fbx_path_str = fbx_path.to_string_lossy().to_string();

                // Python script to import the FBX file
                let import_script = format!(
                    "import bpy; bpy.ops.wm.read_factory_settings(use_empty=True); bpy.ops.import_scene.fbx(filepath=r'{}')",
                    fbx_path_str
                );

                // Handle both regular paths and flatpak commands
                let result = if blender_cmd.starts_with("flatpak run ") {
                    let parts: Vec<&str> = blender_cmd.split_whitespace().collect();
                    std::process::Command::new(parts[0])
                        .args(&parts[1..])
                        .arg("--python-expr")
                        .arg(&import_script)
                        .spawn()
                } else {
                    std::process::Command::new(&blender_cmd)
                        .arg("--python-expr")
                        .arg(&import_script)
                        .spawn()
                };

                match result {
                    Ok(_) => {
                        // Success - Blender is launching
                    }
                    Err(e) => {
                        self.toasts
                            .push(Toast::error(format!("Failed to launch Blender: {}", e)));
                    }
                }
            }
            None => {
                self.toasts.push(Toast::info(
                    "Blender not found. Please install Blender to open FBX files.",
                ));
            }
        }
    }

    /// Scan a generation directory for all associated assets.
    ///
    /// Returns a PipelineOutput with all found assets and a count of how many were found.
    fn scan_generation_directory(dir: &std::path::Path) -> (PipelineOutput, usize) {
        let mut output = PipelineOutput {
            output_dir: Some(dir.to_path_buf()),
            ..Default::default()
        };
        let mut asset_count = 0;

        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    let name_lower = name.to_lowercase();

                    // Check for images
                    if name_lower.ends_with(".png")
                        || name_lower.ends_with(".jpg")
                        || name_lower.ends_with(".jpeg")
                        || name_lower.ends_with(".webp")
                    {
                        // Prefer "image.png" but take any image; only count once
                        let first_image = output.image_path.is_none();
                        if first_image || name == bundle_files::IMAGE {
                            output.image_path = Some(path.clone());
                            if first_image {
                                asset_count += 1;
                            }
                        }
                    }
                    // Check for model.glb (standard filename)
                    if name == bundle_files::MODEL_GLB {
                        output.model_path = Some(path.clone());
                        asset_count += 1;
                    }
                    // Check for model.fbx (standard filename)
                    if name == bundle_files::MODEL_FBX {
                        output.fbx_path = Some(path.clone());
                        asset_count += 1;
                    }
                }
            }
        }

        // Check for textures directory
        let textures_dir = dir.join(bundle_files::TEXTURES_DIR);
        if textures_dir.exists() && textures_dir.is_dir() {
            output.textures_dir = Some(textures_dir);
            asset_count += 1;
        }

        (output, asset_count)
    }

    /// Handle library selection result.
    fn handle_library_selection(&mut self, paths: Vec<PathBuf>) {
        let callback_id = self.library_browser.callback_id.clone();

        match callback_id.as_deref() {
            Some(callback::EXISTING_IMAGE) => {
                // For input selection, just use the specific file
                if let Some(path) = paths.first() {
                    self.existing_image = Some(path.to_string_lossy().to_string());
                }
            }
            Some(callback::PREVIEW_IMAGE)
            | Some(callback::PREVIEW_MODEL)
            | Some(callback::PREVIEW_TEXTURES) => {
                if let Some(path) = paths.first() {
                    // Determine asset type being selected
                    let asset_type_id = match callback_id.as_deref() {
                        Some(callback::PREVIEW_IMAGE) => asset_type::IMAGE,
                        Some(callback::PREVIEW_MODEL) => asset_type::MODEL,
                        Some(callback::PREVIEW_TEXTURES) => asset_type::TEXTURES,
                        _ => asset_type::ASSET,
                    };

                    // Get parent directory (generation bundle directory)
                    // For textures, path IS the textures dir, so parent is the bundle dir
                    // For images/models, path is the file, so parent is also the bundle dir
                    let parent = path.parent();

                    if let Some(parent) = parent {
                        // Scan for all associated assets
                        let (output, _asset_count) = Self::scan_generation_directory(parent);

                        // Determine which associated assets exist
                        let has_image = output.image_path.is_some();
                        let has_model = output.model_path.is_some();
                        let has_textures = output.textures_dir.is_some();

                        // Count OTHER associated assets (not the one being selected)
                        let mut other_assets = views::confirmation_dialog::AssociatedAssets {
                            has_image: false,
                            has_model: false,
                            has_textures: false,
                        };

                        match asset_type_id {
                            asset_type::IMAGE => {
                                other_assets.has_model = has_model;
                                other_assets.has_textures = has_textures;
                            }
                            asset_type::MODEL => {
                                other_assets.has_image = has_image;
                                other_assets.has_textures = has_textures;
                            }
                            asset_type::TEXTURES => {
                                other_assets.has_image = has_image;
                                other_assets.has_model = has_model;
                            }
                            _ => {}
                        }

                        // Show confirmation dialog if there are associated assets and setting is enabled
                        if other_assets.has_any() && self.app_state.show_associated_assets_dialog {
                            self.confirmation_dialog.open(asset_type_id, other_assets);

                            // Store pending load - will be completed when dialog returns result
                            self.pending_bundle_load =
                                Some((output, parent.to_path_buf(), asset_type_id.to_string()));
                        } else {
                            // No associated assets or dialog disabled - load directly
                            self.load_bundle_assets(output, parent, asset_type_id);
                        }
                    } else {
                        // No parent directory, just load the single asset
                        let single_output = match asset_type_id {
                            asset_type::IMAGE => PipelineOutput {
                                image_path: Some(path.clone()),
                                ..Default::default()
                            },
                            asset_type::MODEL => PipelineOutput {
                                model_path: Some(path.clone()),
                                ..Default::default()
                            },
                            asset_type::TEXTURES => PipelineOutput {
                                textures_dir: Some(path.clone()),
                                ..Default::default()
                            },
                            _ => PipelineOutput::default(),
                        };
                        self.output = Some(single_output);
                        self.preview_tab = match asset_type_id {
                            asset_type::IMAGE => PreviewTab::Image,
                            asset_type::MODEL => PreviewTab::Model3D,
                            asset_type::TEXTURES => PreviewTab::Textures,
                            _ => self.preview_tab,
                        };
                    }
                }
            }
            _ => {}
        }
    }

    /// Load bundle assets (called after confirmation or when dialog is disabled).
    fn load_bundle_assets(
        &mut self,
        output: PipelineOutput,
        parent_dir: &std::path::Path,
        primary_asset_type: &str,
    ) {
        // Update app state
        self.app_state.current_generation = Some(parent_dir.to_path_buf());

        // Load bundle metadata into bundle info panel
        if let Err(e) = self.bundle_info_panel.load_bundle(parent_dir.to_path_buf()) {
            tracing::error!("Failed to load bundle metadata: {}", e);
        }

        // Count total assets
        let asset_count = [
            output.image_path.is_some(),
            output.model_path.is_some(),
            output.textures_dir.is_some(),
        ]
        .iter()
        .filter(|&&x| x)
        .count();

        // Set output
        self.output = Some(output);

        // Show success toast
        if asset_count > 1 {
            self.add_toast(Toast::success(format!(
                "Loaded bundle with {} assets",
                asset_count
            )));
        } else {
            self.add_toast(Toast::info(format!("Loaded {}", primary_asset_type)));
        }

        // Set appropriate preview tab
        self.preview_tab = match primary_asset_type {
            asset_type::IMAGE => PreviewTab::Image,
            asset_type::MODEL => PreviewTab::Model3D,
            asset_type::TEXTURES => PreviewTab::Textures,
            _ => self.preview_tab,
        };
    }

    /// Render toast notifications.
    /// Render the clear history confirmation dialog.
    fn render_clear_history_confirmation(&mut self, ctx: &egui::Context) {
        if !self.show_clear_history_confirmation {
            return;
        }

        let mut confirmed = false;
        let mut cancelled = false;

        // Semi-transparent backdrop (no click-outside — user must confirm or cancel)
        views::modal_backdrop(
            ctx,
            "clear_history_backdrop",
            200,
            views::BackdropClick::Block,
        );

        // Dialog window
        egui::Window::new("Clear Prompt History")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_width(400.0);

                // Icon and message
                ui.vertical_centered(|ui| {
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(icons::WARNING)
                            .size(40.0)
                            .color(egui::Color32::from_rgb(255, 180, 100)),
                    );
                    ui.add_space(10.0);
                });

                // Main message
                ui.label(
                    egui::RichText::new("Are you sure you want to clear all prompt history?")
                        .size(14.0),
                );

                ui.add_space(8.0);

                ui.label(
                    egui::RichText::new(format!(
                        "This will permanently delete {} prompt entries from your history.",
                        self.app_state.prompt_history.len()
                    ))
                    .size(13.0)
                    .weak(),
                );

                ui.add_space(12.0);

                ui.label(
                    egui::RichText::new("This action cannot be undone.")
                        .size(12.0)
                        .color(egui::Color32::from_rgb(255, 150, 100)),
                );

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(12.0);

                // Buttons
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Clear button (destructive action)
                        if ui
                            .button(
                                egui::RichText::new(format!("{} Clear History", icons::X))
                                    .size(14.0)
                                    .color(egui::Color32::from_rgb(255, 200, 200)),
                            )
                            .clicked()
                        {
                            confirmed = true;
                        }

                        // Cancel button (primary/safe action)
                        if ui
                            .button(egui::RichText::new("Cancel").size(14.0))
                            .clicked()
                        {
                            cancelled = true;
                        }
                    });
                });

                ui.add_space(8.0);
            });

        // Handle escape key to close
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            cancelled = true;
        }

        // Process actions
        if confirmed {
            self.app_state.prompt_history.clear();
            if let Err(e) = self.app_state.save() {
                tracing::error!("Failed to save state after clearing history: {}", e);
            }
            self.toasts.push(Toast::success("Prompt history cleared"));
            self.show_clear_history_confirmation = false;
        } else if cancelled {
            self.show_clear_history_confirmation = false;
        }
    }

    fn render_toasts(&mut self, ctx: &egui::Context) {
        // Clean up expired toasts
        self.cleanup_toasts();

        if self.toasts.is_empty() {
            return;
        }

        // Render toasts in bottom-right corner
        let screen_rect = ctx.content_rect();
        let toast_width = 280.0;
        let toast_height = 44.0;
        let padding = 16.0;
        let spacing = 8.0;

        for (i, toast) in self.toasts.iter().enumerate() {
            let y_offset = (toast_height + spacing) * i as f32;
            let pos = egui::pos2(
                screen_rect.right() - toast_width - padding,
                screen_rect.bottom() - padding - toast_height - y_offset,
            );

            let opacity = toast.opacity();

            egui::Area::new(egui::Id::new("toast").with(i))
                .fixed_pos(pos)
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    let (bg_color, icon) = match toast.toast_type {
                        ToastType::Info => (egui::Color32::from_rgb(50, 70, 100), icons::INFO),
                        ToastType::Success => (egui::Color32::from_rgb(40, 90, 60), icons::CHECK),
                        ToastType::Error => (egui::Color32::from_rgb(140, 40, 40), icons::X),
                    };

                    egui::Frame::new()
                        .fill(bg_color.gamma_multiply(opacity))
                        .corner_radius(8)
                        .inner_margin(egui::Margin::symmetric(12, 10))
                        .shadow(egui::epaint::Shadow {
                            offset: [0, 2],
                            blur: 8,
                            spread: 0,
                            color: egui::Color32::from_black_alpha((40.0 * opacity) as u8),
                        })
                        .show(ui, |ui| {
                            ui.set_width(toast_width - 24.0);
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(icon)
                                        .size(16.0)
                                        .color(egui::Color32::WHITE.gamma_multiply(opacity)),
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new(&toast.message)
                                        .size(13.0)
                                        .color(egui::Color32::WHITE.gamma_multiply(opacity)),
                                );
                            });
                        });
                });
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Check for completed pipeline
        {
            let mut state = self.state.lock().unwrap();
            if let Some(output) = state.completed_output.take() {
                // Update app state with the new generation
                if let Some(ref dir) = output.output_dir {
                    self.app_state.set_current_generation(Some(dir.clone()));

                    // Load bundle metadata into bundle info panel
                    if let Err(e) = self.bundle_info_panel.load_bundle(dir.clone()) {
                        tracing::error!("Failed to load bundle metadata: {}", e);
                    }
                }
                self.app_state.finish_generation();
                self.current_generation_id = None;

                self.output = Some(output);

                // Refresh bundle list so the new bundle appears in the dropdown
                self.bundle_info_panel
                    .refresh_bundle_list(&self.settings.output_dir);
            }

            // Show error toast once when pipeline fails
            if !self.error_toast_shown && state.error.is_some() && !state.running {
                self.error_toast_shown = true;
                self.toasts.push(Toast::error("Generation failed"));
            }
        }

        // Check for completed file selection
        if let Some(mut rx) = self.pending_file_selection.take() {
            // Try to receive without blocking
            match rx.try_recv() {
                Ok(Some(path)) => {
                    // File was selected
                    self.existing_image = Some(path.to_string_lossy().to_string());
                }
                Ok(None) => {
                    // Dialog was cancelled (no file selected)
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                    // Not ready yet, put it back
                    self.pending_file_selection = Some(rx);
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    // Channel closed without result (shouldn't happen)
                    tracing::warn!("File dialog channel closed unexpectedly");
                }
            }
        }

        // Check for completed export
        if let Some(mut rx) = self.pending_export.take() {
            match rx.try_recv() {
                Ok(Ok(msg)) => {
                    self.add_toast(Toast::success(msg));
                }
                Ok(Err(msg)) => {
                    self.toasts.push(Toast::error(msg));
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                    self.pending_export = Some(rx);
                    ctx.request_repaint();
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    tracing::warn!("Export channel closed unexpectedly");
                }
            }
        }

        // Check for completed bundle import
        if let Some(mut rx) = self.pending_import.take() {
            match rx.try_recv() {
                Ok(Ok(bundle_dir)) => {
                    self.add_toast(Toast::success("Bundle imported"));
                    if let Ok(bundle) = load_bundle(&bundle_dir) {
                        self.output = Some(PipelineOutput::from(bundle));
                        self.app_state.current_generation = Some(bundle_dir.clone());
                        let _ = self.bundle_info_panel.load_bundle(bundle_dir);
                    }
                    self.bundle_info_panel
                        .refresh_bundle_list(&self.settings.output_dir);
                }
                Ok(Err(msg)) => {
                    tracing::error!("Bundle import failed: {}", msg);
                    self.toasts
                        .push(Toast::error(format!("Import failed: {msg}")));
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                    self.pending_import = Some(rx);
                    ctx.request_repaint();
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    tracing::warn!("Import channel closed unexpectedly");
                }
            }
        }

        // Check for completed FBX conversion
        if let Some(mut rx) = self.pending_fbx_conversion.take() {
            match rx.try_recv() {
                Ok(Ok((fbx, textures))) => {
                    self.add_toast(Toast::success("FBX conversion complete"));
                    if let Some(ref mut output) = self.output {
                        output.fbx_path = Some(fbx);
                        if let Some(tex) = textures {
                            output.textures_dir = Some(tex);
                        }
                    }
                }
                Ok(Err(msg)) => {
                    self.toasts.push(Toast::error(msg));
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                    self.pending_fbx_conversion = Some(rx);
                    ctx.request_repaint();
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    tracing::warn!("FBX conversion channel closed unexpectedly");
                }
            }
        }

        // Check for completed demo bundle download
        if let Some(mut rx) = self.pending_demo_download.take() {
            match rx.try_recv() {
                Ok(Ok(asset_tap_core::DemoDownloadResult::Downloaded(demo_dir))) => {
                    self.add_toast(Toast::success("Demo assets downloaded"));
                    if let Ok(bundle) = load_bundle(&demo_dir) {
                        self.output = Some(PipelineOutput::from(bundle));
                        self.app_state.current_generation = Some(demo_dir.clone());
                        let _ = self.bundle_info_panel.load_bundle(demo_dir);
                    }
                    self.bundle_info_panel
                        .refresh_bundle_list(&self.settings.output_dir);
                }
                Ok(Ok(asset_tap_core::DemoDownloadResult::AlreadyExists(v))) => {
                    self.toasts
                        .push(Toast::info(format!("Demo bundle v{v} already downloaded")));
                }
                Ok(Err(msg)) => {
                    tracing::error!("Demo bundle download failed: {}", msg);
                    self.toasts
                        .push(Toast::error("Failed to download demo assets"));
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                    self.pending_demo_download = Some(rx);
                    ctx.request_repaint();
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    tracing::warn!("Demo download channel closed unexpectedly");
                }
            }
        }

        // Update library browser with current output directory
        self.library_browser
            .set_output_dir(self.settings.output_dir.clone());

        // Handle welcome modal (renders over main UI)
        // Skip backdrop if settings modal is also open to avoid double backdrop effect
        if let Some((output_dir, show_on_startup, open_settings)) = self.welcome_modal.render(
            ctx,
            self.settings_modal.is_open,
            self.logo_texture.as_ref(),
            self.pending_demo_download.is_some(),
        ) {
            // Update settings and state from welcome modal
            self.settings.output_dir = output_dir;
            self.app_state.show_welcome_on_startup = show_on_startup;

            // Open settings if user clicked the link (don't save yet, let them configure in settings)
            if open_settings {
                self.settings_modal
                    .open(&self.settings, &self.provider_registry);
            } else {
                // Only save when user clicks "Get Started", not when opening settings
                if let Err(e) = self.settings.save() {
                    tracing::error!("Failed to save settings: {}", e);
                }

                // Start walkthrough for first-time users
                if !self.app_state.has_completed_walkthrough {
                    self.walkthrough.start();
                    self.app_state.has_completed_walkthrough = true;
                }
                let _ = self.app_state.save();
            }
            // Ensure output directory exists
            if let Err(e) = self.settings.ensure_output_dir() {
                tracing::error!("Failed to create output directory: {}", e);
            }
            self.bundle_info_panel
                .refresh_bundle_list(&self.settings.output_dir);
            // Update library browser output dir
            self.library_browser
                .set_output_dir(self.settings.output_dir.clone());
            // Refresh provider registry to pick up new API keys
            self.provider_registry = asset_tap_core::providers::ProviderRegistry::new();
        }

        // Handle demo download request from welcome modal
        if self.welcome_modal.download_requested {
            self.show_demo_download_confirm = true;
        }

        // Demo download confirmation dialog
        if self.show_demo_download_confirm {
            let backdrop_clicked = crate::views::modal_backdrop(
                ctx,
                "demo_download_confirm_backdrop",
                180,
                crate::views::BackdropClick::Close,
            );

            let mut confirmed = false;
            let mut dismissed = backdrop_clicked;

            egui::Window::new("Download Demo Bundle")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.set_width(400.0);
                    ui.add_space(8.0);

                    ui.label(
                        egui::RichText::new(
                            "Download a sample asset bundle with a generated Image and 3D Model?",
                        )
                        .size(14.0),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "This will download approximately {DEMO_BUNDLE_SIZE_LABEL}.",
                        ))
                        .size(12.0)
                        .weak(),
                    );

                    ui.add_space(16.0);

                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .button(
                                    egui::RichText::new(format!(
                                        "{} Download",
                                        crate::icons::DOWNLOAD
                                    ))
                                    .size(14.0),
                                )
                                .clicked()
                            {
                                confirmed = true;
                            }
                            if ui
                                .button(egui::RichText::new("Cancel").size(14.0))
                                .clicked()
                            {
                                dismissed = true;
                            }
                        });
                    });

                    ui.add_space(8.0);
                });

            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                dismissed = true;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                confirmed = true;
            }

            if confirmed {
                self.show_demo_download_confirm = false;
                self.start_demo_download();
            } else if dismissed {
                self.show_demo_download_confirm = false;
            }
        }

        // Delete bundle confirmation dialog
        if let Some(ref bundle_path) = self.pending_delete_bundle.clone() {
            let backdrop_clicked = crate::views::modal_backdrop(
                ctx,
                "delete_bundle_confirm_backdrop",
                180,
                crate::views::BackdropClick::Close,
            );

            let mut confirmed = false;
            let mut dismissed = backdrop_clicked;

            let bundle_name = bundle_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("this bundle");

            egui::Window::new("Delete Bundle")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.set_width(400.0);
                    ui.add_space(8.0);

                    ui.label(
                        egui::RichText::new(format!(
                            "Permanently delete \"{}\"?",
                            bundle_name
                        ))
                        .size(14.0)
                        .strong(),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(
                            "This will delete the bundle directory and all its contents. This action cannot be undone.",
                        )
                        .size(12.0)
                        .color(egui::Color32::from_rgb(255, 150, 100)),
                    );

                    ui.add_space(16.0);

                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .button(
                                    egui::RichText::new(format!(
                                        "{} Delete",
                                        crate::icons::TRASH
                                    ))
                                    .size(14.0)
                                    .color(egui::Color32::from_rgb(255, 100, 100)),
                                )
                                .clicked()
                            {
                                confirmed = true;
                            }
                            if ui
                                .button(egui::RichText::new("Cancel").size(14.0))
                                .clicked()
                            {
                                dismissed = true;
                            }
                        });
                    });

                    ui.add_space(8.0);
                });

            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                dismissed = true;
            }
            // Intentionally no Enter-to-confirm for destructive actions

            if confirmed {
                let path = self.pending_delete_bundle.take().unwrap();
                match std::fs::remove_dir_all(&path) {
                    Ok(()) => {
                        self.add_toast(Toast::success("Bundle deleted"));
                        // Clear current bundle if it was the deleted one
                        if self
                            .app_state
                            .current_generation
                            .as_ref()
                            .is_some_and(|p| p == &path)
                        {
                            self.output = None;
                            self.app_state.current_generation = None;
                            self.bundle_info_panel.current_bundle = None;
                        }
                        self.bundle_info_panel
                            .refresh_bundle_list(&self.settings.output_dir);
                    }
                    Err(e) => {
                        tracing::error!("Failed to delete bundle: {}", e);
                        self.toasts
                            .push(Toast::error(format!("Failed to delete: {e}")));
                        self.pending_delete_bundle = None;
                    }
                }
            } else if dismissed {
                self.pending_delete_bundle = None;
            }
        }

        // Menu bar
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Import Bundle...").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Bundle Archive", &["zip"])
                            .pick_file()
                        {
                            self.import_bundle(path);
                        }
                        ui.close();
                    }
                    if ui.button("Open Output Folder").clicked() {
                        crate::app::open_with_system(
                            &self.settings.output_dir,
                            Some(&mut self.toasts),
                        );
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Settings").clicked() {
                        self.settings_modal
                            .open(&self.settings, &self.provider_registry);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button("View", |ui| {
                    if ui.button("Browse Images").clicked() {
                        self.open_library_for_image_preview();
                        ui.close();
                    }
                    if ui.button("Browse Models").clicked() {
                        self.open_library_for_model_preview();
                        ui.close();
                    }
                    if ui.button("Browse Textures").clicked() {
                        self.open_library_for_textures_preview();
                        ui.close();
                    }
                });

                ui.menu_button("Help", |ui| {
                    if ui.button("Quick Tour").clicked() {
                        self.walkthrough.start();
                        ui.close();
                    }
                    if ui.button("Show Welcome Screen").clicked() {
                        self.welcome_modal.open();
                        ui.close();
                    }
                    ui.separator();
                    let demo_downloading = self.pending_demo_download.is_some();
                    let demo_label = if demo_downloading {
                        "Downloading Demo Bundle..."
                    } else {
                        "Download Demo Bundle"
                    };
                    if ui
                        .add_enabled(!demo_downloading, egui::Button::new(demo_label))
                        .clicked()
                    {
                        self.show_demo_download_confirm = true;
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("About").clicked() {
                        self.about_modal.open();
                        ui.close();
                    }
                });
            });
        });

        // Left sidebar - configuration
        egui::SidePanel::left("config_panel")
            .resizable(true)
            .default_width(320.0)
            .min_width(280.0)
            .show(ctx, |ui| {
                views::sidebar::render(self, ui);
            });

        // Bundle info panel - between sidebar and preview
        egui::SidePanel::left("bundle_info_panel")
            .resizable(true)
            .default_width(300.0)
            .min_width(250.0)
            .max_width(400.0)
            .show(ctx, |ui| {
                if let Some(action) = self.bundle_info_panel.render(ui) {
                    match action {
                        views::bundle_info::BundleInfoAction::CopyPrompt(prompt) => {
                            self.prompt = prompt;
                            self.add_toast(Toast::success("Prompt copied to input"));
                        }
                        views::bundle_info::BundleInfoAction::SwitchBundle(path) => {
                            let (output, _) = Self::scan_generation_directory(&path);
                            self.load_bundle_assets(output, &path, asset_type::MODEL);
                        }
                        views::bundle_info::BundleInfoAction::ExportBundle(src, dest) => {
                            let (tx, rx) = tokio::sync::oneshot::channel();
                            self.pending_export = Some(rx);
                            self.add_toast(Toast::info("Exporting bundle..."));
                            self.runtime.spawn(async move {
                                let result = tokio::task::spawn_blocking(move || {
                                    views::bundle_info::export_bundle_zip(&src, &dest)
                                })
                                .await
                                .unwrap_or_else(|e| Err(format!("Export task failed: {}", e)));
                                let _ = tx.send(
                                    result
                                        .map(|count| format!("Bundle exported ({} files)", count)),
                                );
                            });
                        }
                        views::bundle_info::BundleInfoAction::ImportBundle(source) => {
                            self.import_bundle(source);
                        }
                        views::bundle_info::BundleInfoAction::DeleteBundle(path) => {
                            self.pending_delete_bundle = Some(path);
                        }
                        views::bundle_info::BundleInfoAction::RefreshList => {
                            self.bundle_info_panel
                                .refresh_bundle_list(&self.settings.output_dir);
                        }
                    }
                }
            });

        // Bottom panel - progress
        egui::TopBottomPanel::bottom("progress_panel")
            .resizable(true)
            .default_height(300.0)
            .min_height(100.0)
            .show(ctx, |ui| {
                views::progress::render(self, ui);
            });

        // Central panel - preview
        egui::CentralPanel::default().show(ctx, |ui| {
            views::preview::render(self, ui);
        });

        // Render library browser modal (if open)
        if let Some(selected_paths) = self.library_browser.render(ctx) {
            self.handle_library_selection(selected_paths);
        }

        // Render settings modal (if open)
        if self
            .settings_modal
            .render(ctx, &mut self.settings, &self.provider_registry)
        {
            // Settings were saved - update library browser output dir
            self.library_browser
                .set_output_dir(self.settings.output_dir.clone());
            // Refresh provider registry to pick up new API keys
            self.provider_registry = asset_tap_core::providers::ProviderRegistry::new();
            // Show success toast
            self.add_toast(Toast::success("Settings saved successfully"));
        }

        // Render about modal (if open)
        self.about_modal.render(ctx, self.logo_texture.as_ref());

        // Render template editor modal (if open)
        views::template_editor::show_template_editor(ctx, self);

        // Process any loaded thumbnails from background threads before rendering approval modal
        if self.texture_cache.process_loaded(ctx) {
            ctx.request_repaint();
        }

        // Render image approval modal (if waiting for approval or regenerating)
        let (approval_data, regenerating) = {
            let state = self.state.lock().unwrap();
            (state.awaiting_approval.clone(), state.regenerating_image)
        };
        if approval_data.is_some() || regenerating {
            // Backdrop (no click-outside — user must approve, reject, or regenerate)
            views::modal_backdrop(
                ctx,
                "image_approval_backdrop",
                200,
                views::BackdropClick::Block,
            );

            // Render approval panel as a modal window
            egui::Window::new("Review Generated Image")
                .collapsible(false)
                .resizable(false)
                .fixed_size(egui::vec2(700.0, 0.0))
                .max_height(ctx.content_rect().height() * 0.9)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    if regenerating {
                        // Show loading state while regenerating
                        views::image_approval::render_regenerating(ui);
                    } else if let Some(ref data) = approval_data
                        && let Some(action) = views::image_approval::render(ui, self, data)
                    {
                        match action {
                            views::image_approval::ApprovalAction::Approve => {
                                self.approve_generated_image();
                            }
                            views::image_approval::ApprovalAction::Reject => {
                                self.reject_generated_image();
                            }
                            views::image_approval::ApprovalAction::Regenerate => {
                                self.regenerate_image();
                            }
                        }
                    }
                });
        }

        // Render confirmation dialog (if open)
        let (dialog_result, dont_show_again) = self.confirmation_dialog.render(ctx);
        if let Some(confirmed) = dialog_result {
            if dont_show_again && confirmed {
                // User checked "don't show again" - save preference
                self.app_state.show_associated_assets_dialog = false;
                let _ = self.app_state.save();
            }

            // Process the pending bundle load
            if confirmed {
                // User confirmed - load the bundle
                if let Some((output, parent_dir, asset_type)) = self.pending_bundle_load.take() {
                    self.load_bundle_assets(output, &parent_dir, &asset_type);
                }
            } else {
                // User cancelled - just clear pending load
                self.pending_bundle_load = None;
            }
        }

        // Render clear history confirmation dialog
        self.render_clear_history_confirmation(ctx);

        // Render toast notifications
        self.render_toasts(ctx);

        // Render walkthrough overlay (must be last to draw on top of everything)
        self.walkthrough.render(ctx);

        // Request repaint while pipeline is running or toasts are visible
        if self.state.lock().unwrap().running || !self.toasts.is_empty() {
            ctx.request_repaint();
        }
    }

    fn on_exit(&mut self, _gl: Option<&glow::Context>) {
        // Save current state for session recovery
        self.app_state.preview_tab = match self.preview_tab {
            PreviewTab::Image => "Image".to_string(),
            PreviewTab::Model3D => "Model3D".to_string(),
            PreviewTab::Textures => "Textures".to_string(),
        };

        // Save current generation being viewed
        if let Some(ref output) = self.output {
            self.app_state.current_generation = output.output_dir.clone();
        }

        // Save last prompt
        if !self.prompt.is_empty() {
            self.app_state.last_prompt = Some(self.prompt.clone());
        }

        // Clear in-progress generation (if any was running, it's now interrupted)
        self.app_state.in_progress_generation = None;

        // Persist state
        if let Err(e) = self.app_state.save() {
            tracing::error!("Failed to save app state on exit: {}", e);
        }

        // Mark any running generation as interrupted in history
        if let Some(ref gen_id) = self.current_generation_id {
            let mut history = self.history.lock().unwrap();
            history.cancel_generation(gen_id);
        }
    }
}
