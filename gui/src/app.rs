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
    settings::{LoadStatus, Settings, is_dev_mode},
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

/// Build the list of toasts to seed `App.toasts` with at startup, based on
/// how the settings load went.
///
/// Extracted from `App::new` so we can unit-test the message-and-variant
/// mapping without standing up a real eframe runtime + glow context. The
/// CLI has its own equivalent of this in `cli/src/main.rs` (writes to
/// stderr instead of pushing toasts) — keep the two in rough sync if you
/// add new `LoadStatus` variants.
fn build_startup_toasts(status: &LoadStatus) -> Vec<Toast> {
    match status {
        LoadStatus::Ok => Vec::new(),
        LoadStatus::InitialCreateFailed {
            settings_path,
            error,
        } => vec![Toast::error(format!(
            "Couldn't create your settings file at {}: {}. Anything \
             you change this session won't persist until the underlying \
             problem (likely permissions or disk space) is resolved.",
            settings_path.display(),
            error
        ))],
        LoadStatus::RecoveredFromCorrupt { quarantined_to } => vec![Toast::error(format!(
            "Your settings file was corrupt and couldn't be read. \
             The original has been preserved at {} so you can recover \
             it by hand. A fresh settings.json with defaults will be \
             saved when you change anything.",
            quarantined_to.display()
        ))],
        LoadStatus::CorruptAndInPlace { settings_path } => vec![Toast::error(format!(
            "Your settings file at {} is corrupt and couldn't be moved \
             aside automatically. Running with defaults. The next time \
             anything you change is saved, the corrupt file will be \
             moved to settings.json.bak — copy it somewhere safe first \
             if you want to recover any old values from it.",
            settings_path.display()
        ))],
        LoadStatus::UnreadableFile { settings_path } => vec![Toast::error(format!(
            "Couldn't read your settings file at {}. Running with \
             defaults for this session.",
            settings_path.display()
        ))],
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
    /// 3D Model — leftmost in the tab bar, the default for fresh state and
    /// the auto-selection target when a generation finishes with a model.
    #[default]
    Model3D,
    Image,
    Textures,
}

/// Pick the most-derived preview tab whose underlying asset actually exists
/// in the given pipeline output.
///
/// "Most-derived" means: prefer 3D model > image > textures, walking down the
/// pipeline stages until we find one with content. Used after a generation
/// completes (or after a bundle is loaded from disk) so the UI lands on the
/// most informative tab automatically — usually 3D, but for partial bundles
/// (e.g., a run that errored out before 3D generation, or an image-only
/// bundle imported from elsewhere) we fall back to whatever IS present.
///
/// Returns `None` only if the output has no preview-able asset at all, in
/// which case the caller should leave `preview_tab` unchanged.
pub fn pick_preview_tab_for_output(
    output: &asset_tap_core::types::PipelineOutput,
) -> Option<PreviewTab> {
    // Either format counts as "we have a 3D model" — fbx_path is set when
    // Blender conversion ran, model_path is the GLB and is set whenever the
    // image-to-3D stage completed.
    if output.model_path.is_some() || output.fbx_path.is_some() {
        Some(PreviewTab::Model3D)
    } else if output.image_path.is_some() {
        Some(PreviewTab::Image)
    } else if output.textures_dir.is_some() {
        Some(PreviewTab::Textures)
    } else {
        None
    }
}

impl App {
    /// Create a new application instance.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Get the glow context if available
        let gl_context = cc.gl.clone();

        // Load settings from disk, capturing whether the file was corrupt so we
        // can surface it as a startup toast. Without this, corruption is only
        // visible in the tracing log, which non-technical users will never see.
        let (mut settings, settings_load_status) = Settings::load_with_status();
        let startup_toasts = build_startup_toasts(&settings_load_status);

        // Get default provider and models from registry
        // IMPORTANT: Create registry once and reuse it to avoid performance issues
        let provider_registry = asset_tap_core::providers::ProviderRegistry::new();

        // In dev mode, sync FROM environment TO settings first (so .env keys are picked up)
        if is_dev_mode() {
            settings.sync_from_env(&provider_registry);
            // Ensure dev mode uses .dev/output (in case settings.json has wrong value)
            settings.output_dir = PathBuf::from(".dev/output");
        }

        // Push API keys from settings into env so providers can read them.
        // We use the non-authoritative variant here (set-only, never remove)
        // because this is the startup path: env vars set by .env or some other
        // means must be preserved when settings is empty. The authoritative
        // variant runs only from the settings dialog's save handler, where the
        // user has explicitly cleared a key and expects it to take effect.
        settings.sync_to_env(&provider_registry);

        // Now that settings are loaded and env is populated, surface a warning
        // for any provider that's still unconfigured. We deliberately do this
        // AFTER sync_to_env so the check is accurate — at registry construction
        // time, settings.json hadn't been read yet, and the result would be a
        // false alarm for users with GUI-saved keys.
        provider_registry.log_unconfigured_providers();

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

        // Restore preview tab from saved state, falling back to 3D Model
        // for fresh state and unknown values.
        let restored_preview_tab = match app_state.preview_tab.as_str() {
            "Image" => PreviewTab::Image,
            "Textures" => PreviewTab::Textures,
            _ => PreviewTab::Model3D,
        };

        // Try to restore the current generation being viewed using standardized bundle loading
        let output = app_state.current_generation.as_ref().and_then(|dir| {
            // Use the core bundle loading logic for consistent file discovery
            load_bundle(dir).ok().map(PipelineOutput::from)
        });

        // If the saved tab points at an asset that isn't actually present in
        // the restored output (e.g., user was on the Image tab last session,
        // but the restored bundle only has a 3D model), fall back to the
        // most-derived tab whose asset IS present. Without this, the user
        // can land on a blank tab on cold start.
        let preview_tab = match (&output, restored_preview_tab) {
            (Some(out), tab) => {
                let tab_has_asset = match tab {
                    PreviewTab::Model3D => out.model_path.is_some() || out.fbx_path.is_some(),
                    PreviewTab::Image => out.image_path.is_some(),
                    PreviewTab::Textures => out.textures_dir.is_some(),
                };
                if tab_has_asset {
                    tab
                } else {
                    pick_preview_tab_for_output(out).unwrap_or(tab)
                }
            }
            (None, tab) => tab,
        };

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
            toasts: startup_toasts,
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
                        if let Some(tab) = pick_preview_tab_for_output(&single_output) {
                            self.preview_tab = tab;
                        }
                        self.output = Some(single_output);
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

        // Pick the most-derived preview tab (3D > image > textures) BEFORE
        // moving `output` into self.output below. We prefer this over the
        // `primary_asset_type` string because the string is just a label —
        // the actual decision should be "what's the highest pipeline stage
        // this bundle reached?"
        let tab_for_output = pick_preview_tab_for_output(&output);

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

        if let Some(tab) = tab_for_output {
            self.preview_tab = tab;
        }
    }

    /// Make `bundle_dir` the currently-active bundle: load it from disk,
    /// install it as `self.output`, point `app_state.current_generation` at
    /// it, refresh the bundle dropdown so it appears, and switch the preview
    /// tab to whichever asset is most-derived in the bundle.
    ///
    /// Used by the three "we just produced or pulled a bundle from disk and
    /// want to show it" code paths in `update()` — generation-failure
    /// recovery (the rejection-leaves-an-image case), zip import, and demo
    /// bundle download. Without this helper, all three would (and previously
    /// did) carry near-identical 8-line blocks that diverged any time one
    /// branch was updated and the others weren't.
    ///
    /// Returns `true` on success, `false` if the bundle on disk couldn't be
    /// parsed. Failures are logged via tracing — callers don't need to do
    /// their own error handling unless they want to act on the failure.
    fn activate_bundle_from_dir(&mut self, bundle_dir: PathBuf) -> bool {
        match load_bundle(&bundle_dir) {
            Ok(bundle) => {
                let output = PipelineOutput::from(bundle);
                if let Some(tab) = pick_preview_tab_for_output(&output) {
                    self.preview_tab = tab;
                }
                self.output = Some(output);
                self.app_state
                    .set_current_generation(Some(bundle_dir.clone()));
                if let Err(e) = self.bundle_info_panel.load_bundle(bundle_dir.clone()) {
                    tracing::warn!(
                        "Failed to load bundle metadata for {}: {}",
                        bundle_dir.display(),
                        e
                    );
                }
                self.bundle_info_panel
                    .refresh_bundle_list(&self.settings.output_dir);
                true
            }
            Err(e) => {
                tracing::warn!("Failed to load bundle from {}: {}", bundle_dir.display(), e);
                false
            }
        }
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

        // Layout constants. Toast width is wide enough to comfortably hold a
        // paragraph-length message (e.g. the corrupt-settings.json warning,
        // which is ~250 chars) without overflowing the viewport on a normal
        // window. Height is intentionally NOT fixed — each toast frame grows
        // vertically with its wrapped content, which is the only way to
        // handle multi-line messages without truncation.
        let toast_width: f32 = 460.0;
        let padding = 16.0;
        let spacing = 8.0;

        // Stack all toasts in a single bottom-right anchored Area. egui's
        // vertical layout handles per-toast height naturally — we don't have
        // to precompute heights or track y-offsets. Newest toast is rendered
        // last so it sits closest to the anchor (visually "on top of" older
        // toasts in stacking order, but at the bottom of the column).
        egui::Area::new(egui::Id::new("toast-stack"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-padding, -padding))
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                ui.set_max_width(toast_width);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = spacing;
                    for toast in &self.toasts {
                        Self::render_single_toast(ui, toast, toast_width);
                    }
                });
            });
    }

    /// Render a single toast frame inside an existing vertical layout.
    ///
    /// Width is fixed (`toast_width`), height grows to fit wrapped content.
    /// The icon sits in a fixed-width left column and the message wraps
    /// inside the remaining space — without the explicit column split, a
    /// horizontal layout would let the label occupy whatever's left after
    /// the icon, which makes wrap behavior fragile.
    fn render_single_toast(ui: &mut egui::Ui, toast: &Toast, toast_width: f32) {
        let opacity = toast.opacity();

        let (bg_color, icon) = match toast.toast_type {
            ToastType::Info => (egui::Color32::from_rgb(50, 70, 100), icons::INFO),
            ToastType::Success => (egui::Color32::from_rgb(40, 90, 60), icons::CHECK),
            ToastType::Error => (egui::Color32::from_rgb(140, 40, 40), icons::X),
        };

        // Fixed inner margin so the visual frame thickness matches whether
        // the content is one line or six.
        let h_margin = 12.0;
        let icon_col_width = 22.0;
        let icon_gap = 6.0;
        // Width available for the wrapped message text. Subtract both inner
        // margins, the icon column, and the gap between icon and text.
        let text_width = toast_width - (h_margin * 2.0) - icon_col_width - icon_gap;

        egui::Frame::new()
            .fill(bg_color.gamma_multiply(opacity))
            .corner_radius(8)
            .inner_margin(egui::Margin::symmetric(h_margin as i8, 10))
            .shadow(egui::epaint::Shadow {
                offset: [0, 2],
                blur: 8,
                spread: 0,
                color: egui::Color32::from_black_alpha((40.0 * opacity) as u8),
            })
            .show(ui, |ui| {
                ui.set_width(toast_width - (h_margin * 2.0));
                ui.horizontal_top(|ui| {
                    // Icon column — fixed width so the message text always
                    // wraps inside the same horizontal slot, regardless of
                    // which icon (some are wider than others in a monospace
                    // icon font).
                    ui.allocate_ui_with_layout(
                        egui::vec2(icon_col_width, 0.0),
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| {
                            ui.label(
                                egui::RichText::new(icon)
                                    .size(16.0)
                                    .color(egui::Color32::WHITE.gamma_multiply(opacity)),
                            );
                        },
                    );
                    ui.add_space(icon_gap);
                    // Message column — wrapped to the remaining width.
                    ui.allocate_ui_with_layout(
                        egui::vec2(text_width, 0.0),
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| {
                            ui.set_max_width(text_width);
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&toast.message)
                                        .size(13.0)
                                        .color(egui::Color32::WHITE.gamma_multiply(opacity)),
                                )
                                .wrap(),
                            );
                        },
                    );
                });
            });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Captured under the state lock and processed after release; see the
        // error-toast branch below for context.
        let mut pending_recovery_bundle: Option<PathBuf> = None;

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

                // Auto-select the most-derived preview tab whose asset is
                // present. Usually 3D, but for partial bundles (image-only,
                // or runs that errored before 3D generation completed) we
                // fall back to whatever's actually there. Computed before
                // the move into self.output below.
                if let Some(tab) = pick_preview_tab_for_output(&output) {
                    self.preview_tab = tab;
                }

                self.output = Some(output);

                // Refresh bundle list so the new bundle appears in the dropdown
                self.bundle_info_panel
                    .refresh_bundle_list(&self.settings.output_dir);
            }

            // Show error toast once when pipeline fails
            if !self.error_toast_shown && state.error.is_some() && !state.running {
                self.error_toast_shown = true;
                self.toasts.push(Toast::error("Generation failed"));

                // If the failure left a partial bundle on disk (e.g., the
                // user rejected the image after the text-to-image stage
                // succeeded), capture the bundle dir while we still hold
                // the state lock. We process it below the lock release
                // because activate_bundle_from_dir takes &mut self, which
                // conflicts with the lock guard's borrow of self.
                pending_recovery_bundle = state
                    .recovery_info
                    .as_ref()
                    .and_then(|r| r.image_path.parent().map(|p| p.to_path_buf()));
            }
        }
        // Lock released. If we captured a recovery bundle above, surface
        // it now — refreshes the dropdown so the new partial bundle appears
        // and switches the preview tab to the most-derived asset present.
        // Without this, the user would have to manually click "Refresh" to
        // find the bundle they just generated.
        if let Some(bundle_dir) = pending_recovery_bundle {
            self.activate_bundle_from_dir(bundle_dir);
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
                    self.activate_bundle_from_dir(bundle_dir);
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
                    self.activate_bundle_from_dir(demo_dir);
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

#[cfg(test)]
mod tests {
    use super::{PreviewTab, ToastType, build_startup_toasts, pick_preview_tab_for_output};
    use asset_tap_core::settings::LoadStatus;
    use asset_tap_core::types::PipelineOutput;
    use std::path::PathBuf;

    /// `Ok` is the happy path: zero toasts, nothing to surface.
    #[test]
    fn test_build_startup_toasts_ok_yields_nothing() {
        let toasts = build_startup_toasts(&LoadStatus::Ok);
        assert!(toasts.is_empty(), "Ok status must not produce any toasts");
    }

    /// Each non-Ok variant must produce exactly one error toast whose message
    /// includes the path the user needs to know about. We assert on substrings
    /// rather than exact text so harmless wording tweaks don't break the test
    /// — but the path itself is non-negotiable, since it's the only piece of
    /// the message that's actually actionable for the user.
    #[test]
    fn test_build_startup_toasts_initial_create_failed() {
        let path = PathBuf::from("/some/readonly/dir/settings.json");
        let toasts = build_startup_toasts(&LoadStatus::InitialCreateFailed {
            settings_path: path.clone(),
            error: "Permission denied (os error 13)".to_string(),
        });

        assert_eq!(toasts.len(), 1);
        let toast = &toasts[0];
        assert!(matches!(toast.toast_type, ToastType::Error));
        assert!(
            toast.message.contains(&path.display().to_string()),
            "message should mention the settings path; got {:?}",
            toast.message
        );
        assert!(
            toast.message.contains("Permission denied"),
            "message should include the underlying OS error; got {:?}",
            toast.message
        );
    }

    #[test]
    fn test_build_startup_toasts_recovered_from_corrupt() {
        let quarantine = PathBuf::from("/cfg/settings.json.corrupt-1234");
        let toasts = build_startup_toasts(&LoadStatus::RecoveredFromCorrupt {
            quarantined_to: quarantine.clone(),
        });

        assert_eq!(toasts.len(), 1);
        let toast = &toasts[0];
        assert!(matches!(toast.toast_type, ToastType::Error));
        assert!(
            toast.message.contains(&quarantine.display().to_string()),
            "message should tell the user where the quarantined file is; got {:?}",
            toast.message
        );
        assert!(
            toast.message.to_lowercase().contains("corrupt"),
            "message should explicitly use the word 'corrupt'; got {:?}",
            toast.message
        );
    }

    #[test]
    fn test_build_startup_toasts_corrupt_and_in_place() {
        let path = PathBuf::from("/cfg/settings.json");
        let toasts = build_startup_toasts(&LoadStatus::CorruptAndInPlace {
            settings_path: path.clone(),
        });

        assert_eq!(toasts.len(), 1);
        let toast = &toasts[0];
        assert!(matches!(toast.toast_type, ToastType::Error));
        assert!(toast.message.contains(&path.display().to_string()));
        // The previously-buggy copy claimed "changes will NOT be saved"; the
        // current copy correctly warns that the corrupt file will end up in
        // .bak on the next save. Guard against a regression to the wrong copy.
        assert!(
            toast.message.contains(".bak"),
            "message must explain that the corrupt file moves to .bak on next save; got {:?}",
            toast.message
        );
        assert!(
            !toast.message.contains("NOT be saved"),
            "message must NOT claim changes won't be saved — they will; got {:?}",
            toast.message
        );
    }

    #[test]
    fn test_build_startup_toasts_unreadable_file() {
        let path = PathBuf::from("/cfg/settings.json");
        let toasts = build_startup_toasts(&LoadStatus::UnreadableFile {
            settings_path: path.clone(),
        });

        assert_eq!(toasts.len(), 1);
        let toast = &toasts[0];
        assert!(matches!(toast.toast_type, ToastType::Error));
        assert!(toast.message.contains(&path.display().to_string()));
    }

    /// All non-Ok variants must produce error toasts (not info or success).
    /// This is a smoke test that catches accidental severity downgrades —
    /// e.g., someone replacing `Toast::error` with `Toast::info` and the
    /// user no longer noticing their settings just got nuked.
    #[test]
    fn test_build_startup_toasts_all_failures_are_errors() {
        let path = PathBuf::from("/p");
        let cases = [
            LoadStatus::InitialCreateFailed {
                settings_path: path.clone(),
                error: "x".to_string(),
            },
            LoadStatus::RecoveredFromCorrupt {
                quarantined_to: path.clone(),
            },
            LoadStatus::CorruptAndInPlace {
                settings_path: path.clone(),
            },
            LoadStatus::UnreadableFile {
                settings_path: path.clone(),
            },
        ];
        for status in cases {
            let toasts = build_startup_toasts(&status);
            assert_eq!(toasts.len(), 1, "expected exactly one toast for {status:?}");
            assert!(
                matches!(toasts[0].toast_type, ToastType::Error),
                "expected Error severity for {status:?}, got {:?}",
                toasts[0].toast_type
            );
        }
    }

    // =========================================================================
    // pick_preview_tab_for_output — auto-select the right preview tab based
    // on which assets are actually present in a PipelineOutput.
    // =========================================================================

    /// Default constructor uses Default::default() which gives an empty output
    /// with all paths None. Helper to keep tests terse.
    fn empty_output() -> PipelineOutput {
        PipelineOutput::default()
    }

    /// A bundle that made it all the way through 3D generation should land on
    /// the 3D Model tab regardless of what other assets are also present.
    /// This is the dominant case for fresh successful generations.
    #[test]
    fn test_pick_preview_tab_full_pipeline_picks_model3d() {
        let mut out = empty_output();
        out.image_path = Some(PathBuf::from("/x/image.png"));
        out.model_path = Some(PathBuf::from("/x/model.glb"));
        out.textures_dir = Some(PathBuf::from("/x/textures"));
        assert_eq!(pick_preview_tab_for_output(&out), Some(PreviewTab::Model3D));
    }

    /// FBX-only outputs (Blender ran but the GLB got cleaned up, or some
    /// alternate flow) still count as having a 3D model.
    #[test]
    fn test_pick_preview_tab_fbx_only_picks_model3d() {
        let mut out = empty_output();
        out.image_path = Some(PathBuf::from("/x/image.png"));
        out.fbx_path = Some(PathBuf::from("/x/model.fbx"));
        assert_eq!(pick_preview_tab_for_output(&out), Some(PreviewTab::Model3D));
    }

    /// The partial-bundle case the user explicitly asked for: a run that
    /// errored out before reaching 3D, leaving only an image. Falling back
    /// to 3D would land on a blank tab — fall back to Image instead.
    #[test]
    fn test_pick_preview_tab_image_only_picks_image() {
        let mut out = empty_output();
        out.image_path = Some(PathBuf::from("/x/image.png"));
        assert_eq!(pick_preview_tab_for_output(&out), Some(PreviewTab::Image));
    }

    /// Textures-only is the lowest fallback. Vanishingly rare in practice
    /// (you'd have to extract textures from an existing model and discard
    /// everything else) but supported for completeness.
    #[test]
    fn test_pick_preview_tab_textures_only_picks_textures() {
        let mut out = empty_output();
        out.textures_dir = Some(PathBuf::from("/x/textures"));
        assert_eq!(
            pick_preview_tab_for_output(&out),
            Some(PreviewTab::Textures)
        );
    }

    /// An empty output (no assets at all) returns None so the caller leaves
    /// the current tab alone. The caller's existing `if let Some(tab) = ...`
    /// pattern relies on this.
    #[test]
    fn test_pick_preview_tab_empty_returns_none() {
        assert_eq!(pick_preview_tab_for_output(&empty_output()), None);
    }

    /// 3D model takes priority over image when both are present. Belt-and-
    /// suspenders given the dominant case test above — this isolates the
    /// model > image precedence rule from the also-have-textures noise.
    #[test]
    fn test_pick_preview_tab_model_beats_image() {
        let mut out = empty_output();
        out.image_path = Some(PathBuf::from("/x/image.png"));
        out.model_path = Some(PathBuf::from("/x/model.glb"));
        assert_eq!(pick_preview_tab_for_output(&out), Some(PreviewTab::Model3D));
    }

    /// Image takes priority over textures when both are present (no model).
    #[test]
    fn test_pick_preview_tab_image_beats_textures() {
        let mut out = empty_output();
        out.image_path = Some(PathBuf::from("/x/image.png"));
        out.textures_dir = Some(PathBuf::from("/x/textures"));
        assert_eq!(pick_preview_tab_for_output(&out), Some(PreviewTab::Image));
    }

    /// `PreviewTab::default()` should be `Model3D` because that's the
    /// leftmost tab in the visual tab bar (`gui/src/views/preview.rs`)
    /// and the user-stated preferred default. If anyone reorders the enum
    /// variants without thinking, this catches it.
    #[test]
    fn test_preview_tab_default_is_model3d() {
        assert_eq!(PreviewTab::default(), PreviewTab::Model3D);
    }
}
