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

/// Fallback locator for a failed run's saved image when the pipeline didn't
/// report its own generation directory (see `run_pipeline`'s `gen_dir_out`).
///
/// Scans `output_dir` for timestamp-format subdirectories (`YYYY-MM-DD_HHMMSS`)
/// only, and returns `image.png` from the newest one (by parsed timestamp, not
/// lexicographic order). Non-timestamp directories are ignored.
///
/// Limitation: this is a best-effort heuristic. If two runs land in the same
/// output directory concurrently it could still pick the sibling run's image.
/// The `gen_dir_out` path is authoritative; this only runs if it's unavailable.
fn latest_run_image(output_dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let entries = std::fs::read_dir(output_dir).ok()?;
    let mut candidates: Vec<(chrono::NaiveDateTime, std::path::PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name();
            let name = name.to_str()?;
            let ts = chrono::NaiveDateTime::parse_from_str(name, "%Y-%m-%d_%H%M%S").ok()?;
            Some((ts, e.path()))
        })
        .collect();
    // Newest timestamp first.
    candidates.sort_by_key(|b| std::cmp::Reverse(b.0));
    candidates
        .into_iter()
        .map(|(_, dir)| dir.join(bundle_files::IMAGE))
        .find(|p| p.exists())
}

/// Reconcile a persisted (provider, model) selection against the live registry.
///
/// - Both valid → returned unchanged.
/// - Provider valid but model doesn't belong to it → keep provider, pick that
///   provider's default model.
/// - Provider missing → fall back to `default_provider` and its default model.
///
/// Used at startup to recover from a removed provider YAML, a renamed model,
/// or any other change that would leave the sidebar pointing at a dead
/// reference until the user manually clicks a dropdown.
fn reconcile_provider_selection(
    registry: &asset_tap_core::providers::ProviderRegistry,
    capability: asset_tap_core::providers::ProviderCapability,
    provider_id: String,
    model_id: String,
    default_provider_id: String,
) -> (String, String) {
    if let Some(provider) = registry.get(&provider_id) {
        let has_model = provider
            .list_models(capability)
            .iter()
            .any(|m| m.id == model_id);
        if has_model {
            return (provider_id, model_id);
        }
        let fallback_model = provider
            .get_default_model(capability)
            .ok()
            .map(|m| m.id)
            .unwrap_or_default();
        return (provider_id, fallback_model);
    }
    // Provider gone — fall back to the registry default and its default model.
    let fallback_model = registry
        .get(&default_provider_id)
        .and_then(|p| p.get_default_model(capability).ok())
        .map(|m| m.id)
        .unwrap_or_default();
    (default_provider_id, fallback_model)
}

/// Whether an image reference points at a remote URL rather than a local file.
///
/// Mirrors the CLI's check so both surfaces agree on which inputs are validated
/// against the filesystem.
fn is_remote_url(image: &str) -> bool {
    image.starts_with("http://") || image.starts_with("https://")
}

/// Whether the current selections describe a run with no work in it.
///
/// An input image replaces the image-generation stage and "image only" removes
/// the 3D stage, so together they leave nothing to execute. The CLI rejects the
/// same pair outright (`--image` with `--image-only`).
///
/// Neither selection is silently undone — both are the user's, and quietly
/// clearing one hides the mistake rather than surfacing it. The sidebar reports
/// the conflict and Generate stays disabled until the user resolves it.
pub(crate) fn is_no_op_run(skip_3d: bool, has_existing_image: bool) -> bool {
    skip_3d && has_existing_image
}

/// The file name for a toast, or the whole path when there is none.
fn file_label(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn bundle_import_combined_exts() -> Vec<&'static str> {
    let mut exts = Vec::with_capacity(3 + asset_tap_core::constants::files::IMAGE_EXTS.len());
    exts.extend(["zip", "json", "glb"]);
    exts.extend(asset_tap_core::constants::files::IMAGE_EXTS.iter().copied());
    exts
}

/// File picker shared by File → Import Bundle and the bundle-info import button.
pub(crate) fn pick_bundle_import_file() -> Option<std::path::PathBuf> {
    let combined = bundle_import_combined_exts();
    rfd::FileDialog::new()
        .add_filter("Bundle, model, or image", &combined)
        .add_filter("3D model (GLB)", &["glb"])
        .add_filter("Images", asset_tap_core::constants::files::IMAGE_EXTS)
        .add_filter("ZIP archive", &["zip"])
        .pick_file()
}

/// File picker shared by File → Install Animation Pack and Animate → Add pack.
pub(crate) fn pick_pack_install_file() -> Option<std::path::PathBuf> {
    rfd::FileDialog::new()
        .set_title("Animation pack")
        .add_filter("Animation pack", &["zip", "glb", "gltf"])
        .pick_file()
}

/// Folder picker shared by File → Install Animation Pack Folder and Add pack.
pub(crate) fn pick_pack_install_folder() -> Option<std::path::PathBuf> {
    rfd::FileDialog::new()
        .set_title("Animation pack folder")
        .pick_folder()
}

pub(crate) fn pick_image_file() -> Option<std::path::PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Images", asset_tap_core::constants::files::IMAGE_EXTS)
        .pick_file()
}

pub(crate) fn pick_glb_file() -> Option<std::path::PathBuf> {
    rfd::FileDialog::new()
        .add_filter("3D model (GLB)", &["glb"])
        .pick_file()
}

/// True when the pointer is over `rect`, during an OS file drag included —
/// but only because [`crate::dnd::inject_drag_pointer`] supplies the position
/// winit withholds for the duration of a drag. Without that hook this is
/// `false` for every zone on every platform, which is what makes drop zones
/// look mysteriously dead.
pub(crate) fn pointer_over_rect(ctx: &egui::Context, rect: egui::Rect) -> bool {
    ctx.pointer_latest_pos().is_some_and(|p| rect.contains(p))
}

/// Hovered OS-drag paths whose pointer is over `rect`.
pub(crate) fn hovered_paths_over(ctx: &egui::Context, rect: egui::Rect) -> Vec<std::path::PathBuf> {
    if !pointer_over_rect(ctx, rect) {
        return Vec::new();
    }
    ctx.input(|i| {
        i.raw
            .hovered_files
            .iter()
            .filter_map(|f| f.path.clone())
            .collect()
    })
}

/// This frame's dropped paths, if the pointer is over `rect`. Read-only:
/// pair with [`claim_dropped`] to take the ones a zone actually uses.
fn dropped_paths_over(ctx: &egui::Context, rect: egui::Rect) -> Vec<std::path::PathBuf> {
    if !pointer_over_rect(ctx, rect) {
        return Vec::new();
    }
    ctx.input(|i| {
        i.raw
            .dropped_files
            .iter()
            .filter_map(|f| f.path.clone())
            .collect()
    })
}

/// Remove `claimed` from this frame's dropped files so later zones cannot
/// also take them. Anything left is what [`App::drop_unclaimed`] sees at the
/// end of the frame, which is why a zone claims only the files it will use:
/// the rest must still reach the toast that says where they belong. Must run
/// during [`eframe::App::ui`].
fn claim_dropped(ctx: &egui::Context, claimed: &[std::path::PathBuf]) {
    if claimed.is_empty() {
        return;
    }
    ctx.input_mut(|i| {
        i.raw.dropped_files.retain(|f| {
            !f.path
                .as_deref()
                .is_some_and(|p| claimed.iter().any(|c| c == p))
        });
    });
}

pub(crate) fn paint_drop_overlay(ctx: &egui::Context, rect: egui::Rect, label: &str) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("drop_zone_overlay"),
    ));
    painter.rect_filled(rect, 0.0, egui::Color32::from_black_alpha(160));
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(22.0),
        egui::Color32::WHITE,
    );
}

/// Embedded logo image for in-app branding (512x512 with "ASSET TAP" text).
const LOGO_BYTES: &[u8] = include_bytes!("../../assets/logo.png");

use crate::constants::{asset_type, callback, clip_packs};
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

type PackInstallResult = Result<(String, String, usize), String>;

/// An import the user asked for, held until the previous one finishes.
enum QueuedImport {
    /// A bundle folder/zip, or a lone `.glb`/image wrapped into one.
    Bundle(std::path::PathBuf),
    /// A still and a mesh dropped together, paired into one bundle.
    Loose(Vec<std::path::PathBuf>),
}

enum WorkbenchDone {
    Fit {
        /// Placeable joints moved > 1 cm from the auto-fit, and the largest move.
        moved_joints: usize,
        max_moved_m: f32,
        /// Animations the file holds after the write.
        clips: Vec<String>,
        /// Baked clips the re-bind could not carry over.
        dropped_clips: Vec<String>,
    },
    Bake {
        /// Exactly what the file holds now; Bake is declarative.
        clips: Vec<String>,
    },
    Seed {
        markers: Vec<asset_tap_core::BindMarker>,
    },
    Preview {
        clip: Box<asset_tap_core::SkinnedClip>,
    },
    /// Everything opening the Animation panel needs to read off disk, in one
    /// trip: the baked set, whether a foreign skin is about to be replaced,
    /// and (unfitted only) the skeleton Rig opens with.
    Opened {
        clips: Vec<String>,
        foreign_joints: Option<usize>,
        markers: Option<Vec<asset_tap_core::BindMarker>>,
    },
}

/// Which model and which request a workbench job was issued for, so a
/// completion that arrives after the user moved on is dropped instead of
/// being applied to whatever is loaded now (VIEWER_ANIMATE.md: "Never
/// silently apply pending edits to another asset").
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkbenchJob {
    /// From `App::workbench_seq`, bumped per job and on every model swap.
    pub id: u64,
    pub model: PathBuf,
}

/// True when a finished job still describes the model on screen and no
/// newer request or model swap has superseded it.
pub(crate) fn workbench_completion_is_current(
    job: &WorkbenchJob,
    current_model: Option<&std::path::Path>,
    latest_id: u64,
) -> bool {
    job.id == latest_id && current_model == Some(job.model.as_path())
}

/// Non-blocking read of a oneshot: the value when it is ready, `None` while
/// it is not (the receiver goes back in `slot` and a repaint is requested so
/// the next frame asks again) or if the sender was dropped (logged; the job
/// is gone).
fn poll_oneshot<T>(
    slot: &mut Option<tokio::sync::oneshot::Receiver<T>>,
    ctx: &egui::Context,
    what: &str,
) -> Option<T> {
    let mut rx = slot.take()?;
    match rx.try_recv() {
        Ok(value) => Some(value),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
            *slot = Some(rx);
            ctx.request_repaint();
            None
        }
        Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
            tracing::warn!("{what} channel closed unexpectedly");
            None
        }
    }
}

/// Split the ticked set into what Bake can actually write (catalog order, so
/// the file matches the list) and what it cannot: names whose pack is not
/// installed. A model baked on another machine, or with a pack since removed,
/// pre-ticks clips nothing can re-source. Bake used to count them, then
/// write only the rest — with none installed it silently became Clear.
pub(crate) fn split_bake_set<'a>(
    bake_set: &std::collections::BTreeSet<String>,
    catalog_ids: impl IntoIterator<Item = &'a str>,
) -> (Vec<String>, Vec<String>) {
    let available: Vec<String> = catalog_ids
        .into_iter()
        .filter(|id| bake_set.contains(*id))
        .map(str::to_string)
        .collect();
    let unavailable = bake_set
        .iter()
        .filter(|id| !available.contains(id))
        .cloned()
        .collect();
    (available, unavailable)
}

/// How a mixed drop of loose meshes and stills becomes bundles: the first
/// mesh pairs with the first still, and every other file gets a bundle of
/// its own. Core's pair import keeps the largest of each kind and deletes
/// the rest, so handing it everything lost files without a word.
pub(crate) fn plan_loose_imports(glbs: Vec<PathBuf>, images: Vec<PathBuf>) -> Vec<Vec<PathBuf>> {
    let mut glbs = glbs.into_iter();
    let mut images = images.into_iter();
    let mut plan = Vec::new();
    match (glbs.next(), images.next()) {
        (Some(g), Some(i)) => plan.push(vec![g, i]),
        (Some(g), None) => plan.push(vec![g]),
        (None, Some(i)) => plan.push(vec![i]),
        (None, None) => {}
    }
    plan.extend(glbs.map(|g| vec![g]));
    plan.extend(images.map(|i| vec![i]));
    plan
}

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
    // Route through the shared LoadStatus::user_message() so the GUI toast and
    // the CLI's stderr warning can't drift apart.
    status
        .user_message()
        .map(|msg| vec![Toast::error(msg)])
        .unwrap_or_default()
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

    /// Clip the Animation panel currently has selected for playback.
    pub clip: String,

    /// Stop after image generation (image-only bundle, no 3D stage).
    pub skip_3d: bool,

    /// Existing image path/URL (skips image generation).
    pub existing_image: Option<String>,

    /// Cached display label for `existing_image`, keyed by its path. Computing
    /// the label reads bundle metadata from disk, so it's memoized here rather
    /// than recomputed every frame while the sidebar is drawn.
    pub existing_image_label: Option<(String, String)>,

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

    /// Glow context for the native `three-d` viewport.
    pub gl_context: Option<Arc<glow::Context>>,

    /// Library browser for selecting past generations.
    pub library_browser: LibraryBrowser,

    /// Texture thumbnail cache with background loading.
    pub texture_cache: TextureCache,

    /// Full-resolution texture for image approval modal (loaded on demand).
    pub approval_texture: Option<(PathBuf, egui::TextureHandle)>,

    /// Path whose approval-image decode failed, so we don't retry it (and
    /// spin the UI at max FPS) every frame. Cleared when the approval texture
    /// is reset for a new image.
    pub approval_texture_failed: Option<PathBuf>,

    /// In-flight fit / bake / preview / open, with the identity it was
    /// issued for. Only one at a time.
    pending_workbench: Option<(
        WorkbenchJob,
        tokio::sync::oneshot::Receiver<Result<WorkbenchDone, String>>,
    )>,

    /// Id of the latest workbench request. Bumped per job and on every model
    /// swap, so a completion carrying an older id is stale by construction.
    workbench_seq: u64,

    /// Optional Animation panel. Off is inspect.
    pub workbench_animate: bool,

    /// Last Bones checkbox in the Animation panel. Default on.
    pub workbench_show_bones: bool,

    /// After Bind, wait for the reload then Preview the current clip so the
    /// new weights are visible without another click.
    /// Clips from every installed pack, cached because the panel reads it
    /// while rendering and `list_clips` touches the filesystem.
    pub clip_catalog: Vec<asset_tap_core::ClipCatalogEntry>,
    /// Clips Bake will write. Seeded from what the model already contains, so
    /// reopening a baked asset shows its set rather than an empty one.
    pub bake_set: std::collections::BTreeSet<String>,
    /// What the model on disk currently holds. Kept beside `bake_set` so the
    /// panel can say what a Bake would add and remove before it runs.
    pub model_clips: std::collections::BTreeSet<String>,
    /// Awaiting confirmation to strip every animation from the model.
    pub pending_clear_animation: bool,
    /// Substring filter over the clip list — 85 rows is too many to scan.
    pub clip_filter: String,

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

    /// Pending demo bundle download result (from async download).
    pending_demo_download:
        Option<tokio::sync::oneshot::Receiver<Result<asset_tap_core::DemoDownloadResult, String>>>,

    /// Pending free Standard clip-pack download.
    pending_clip_packs_download: Option<
        tokio::sync::oneshot::Receiver<Result<asset_tap_core::ClipPacksDownloadResult, String>>,
    >,

    /// Pending bundle import result (from async zip extraction).
    pending_import: Option<tokio::sync::oneshot::Receiver<Result<std::path::PathBuf, String>>>,

    /// Pending attach of an image/GLB into the open bundle.
    pending_attach: Option<tokio::sync::oneshot::Receiver<Result<std::path::PathBuf, String>>>,

    /// Pending clip-pack install (separate from bind/bake so a drop still works).
    pending_pack_install: Option<tokio::sync::oneshot::Receiver<PackInstallResult>>,

    /// Whether to show the demo download confirmation dialog.
    show_demo_download_confirm: bool,

    /// Whether to show the clip-pack download confirmation dialog.
    show_clip_packs_download_confirm: bool,

    /// Bundle path pending deletion (waiting for confirmation).
    pending_delete_bundle: Option<std::path::PathBuf>,

    /// Cursor position during an OS file drag, in egui points. winit reports
    /// no coordinates for a file drag, so [`crate::dnd`] queries the OS and
    /// this carries the last hovered position onto the drop frame.
    drag_pointer: Option<egui::Pos2>,

    /// `is_pack_drop` verdicts for the length of one drag, keyed by path.
    /// Cleared when the drag ends; see [`App::is_pack_drop_cached`].
    pack_drop_cache: std::collections::HashMap<std::path::PathBuf, bool>,

    /// Imports waiting for the one in flight. Each needs its own completion
    /// channel, and `pending_import` holds exactly one, so dropping several
    /// files at once has to queue rather than overwrite.
    queued_imports: std::collections::VecDeque<QueuedImport>,

    /// Pack installs waiting their turn. Serialized against each other *and*
    /// against a bind/bake, because they write into `packs_root()` while the
    /// workbench may be reading a pack out of it.
    queued_pack_installs: std::collections::VecDeque<std::path::PathBuf>,

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
    // `model_path` is the GLB, set when the image-to-3D stage completed.
    if output.model_path.is_some() {
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
            .unwrap_or(default_image_provider.clone());
        let image_model = app_state
            .selected_image_model
            .clone()
            .unwrap_or(default_image_model);
        let model_3d_provider = app_state
            .selected_3d_provider
            .clone()
            .unwrap_or(default_3d_provider.clone());
        let model_3d = app_state
            .selected_3d_model
            .clone()
            .unwrap_or(default_3d_model);

        // Persisted provider/model selections can point at things that aren't
        // currently registered — e.g. mock mode hides non-fal providers, or a
        // user's provider YAML was removed. Fall back to the registry's default
        // rather than handing the pipeline a dead reference.
        let (image_provider, image_model) = reconcile_provider_selection(
            &provider_registry,
            ProviderCapability::TextToImage,
            image_provider,
            image_model,
            default_image_provider,
        );
        let (model_3d_provider, model_3d) = reconcile_provider_selection(
            &provider_registry,
            ProviderCapability::ImageTo3D,
            model_3d_provider,
            model_3d,
            default_3d_provider,
        );

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
                    PreviewTab::Model3D => out.model_path.is_some(),
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
            clip: "walk".into(),
            skip_3d: false,
            existing_image: None,
            existing_image_label: None,

            // State
            state: Arc::new(Mutex::new(PipelineState::default())),
            output,

            // UI
            preview_tab,
            available_templates: list_templates(),
            provider_registry, // Reuse the registry created above
            model_viewer: Arc::new(Mutex::new(ModelViewer::new())),
            gl_context,
            library_browser: LibraryBrowser::new(),
            texture_cache: TextureCache::new(),
            approval_texture: None,
            approval_texture_failed: None,
            bundle_info_panel: views::bundle_info::BundleInfoPanel::new(),
            confirmation_dialog: views::confirmation_dialog::ConfirmationDialog::new(),
            pending_workbench: None,
            workbench_seq: 0,
            workbench_animate: false,
            workbench_show_bones: true,
            clip_catalog: asset_tap_core::list_clips(),
            bake_set: std::collections::BTreeSet::new(),
            model_clips: std::collections::BTreeSet::new(),
            pending_clear_animation: false,
            clip_filter: String::new(),

            // Settings
            settings,
            welcome_modal,
            settings_modal: SettingsModal::new(),
            about_modal: AboutModal::new(),
            logo_texture: None, // Loaded after context is available

            // State & History
            app_state,
            history: Arc::new(Mutex::new(history)),
            current_generation_id: None,

            // Pending Bundle Load
            pending_bundle_load: None,

            // Pending File Dialog
            pending_file_selection: None,
            pending_export: None,
            pending_demo_download: None,
            pending_clip_packs_download: None,
            pending_import: None,
            pending_attach: None,
            pending_pack_install: None,
            show_demo_download_confirm: false,
            show_clip_packs_download_confirm: false,
            pending_delete_bundle: None,
            drag_pointer: None,
            pack_drop_cache: std::collections::HashMap::new(),
            queued_imports: std::collections::VecDeque::new(),
            queued_pack_installs: std::collections::VecDeque::new(),

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

    pub fn request_clip_packs_download(&mut self) {
        self.show_clip_packs_download_confirm = true;
    }

    pub fn clip_packs_downloading(&self) -> bool {
        self.pending_clip_packs_download.is_some()
    }

    fn start_clip_packs_download(&mut self) {
        if self.pending_clip_packs_download.is_some() {
            return;
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending_clip_packs_download = Some(rx);
        self.toasts
            .push(Toast::info("Checking for Universal Animation Libraries..."));
        self.runtime.spawn(async move {
            let result = asset_tap_core::download_clip_packs(false, |_progress| {}).await;
            let _ = tx.send(result.map_err(|e| e.to_string()));
        });
    }

    /// True when the path has an image extension the pipeline accepts.
    pub(crate) fn is_image_file(path: &std::path::Path) -> bool {
        asset_tap_core::constants::files::is_image_path(path)
    }

    pub(crate) fn is_glb_file(path: &std::path::Path) -> bool {
        path.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("glb"))
    }

    /// Refuse to replace `self.output` while something else owns it: a
    /// running pipeline will install its own result, and an in-flight
    /// rig/bake was issued for the model on screen. Toasts the reason.
    fn output_swap_blocked(&mut self) -> bool {
        if self.state.lock().unwrap().running {
            self.add_toast(Toast::info(
                "Generation in progress. The bundle is in the library; open it when the run finishes",
            ));
            return true;
        }
        if self.workbench_busy() {
            self.add_toast(Toast::info(
                "Wait for the current rig or bake to finish before switching bundles",
            ));
            return true;
        }
        false
    }

    /// True for paths that can become a library bundle when dropped on
    /// Bundle Info: folders, zips, GLBs, images, and bundle.json.
    pub(crate) fn is_bundle_drop(path: &std::path::Path) -> bool {
        path.is_dir()
            || Self::is_image_file(path)
            || path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("zip") || e.eq_ignore_ascii_case("glb"))
            || path
                .file_name()
                .is_some_and(|n| n == asset_tap_core::constants::files::bundle::METADATA)
    }

    /// True for a Quaternius zip/folder/GLB (or pack.json) that should install
    /// as an animation pack instead of wrapping into the library.
    pub(crate) fn is_pack_drop(path: &std::path::Path) -> bool {
        asset_tap_core::looks_like_clip_pack(path)
    }

    /// [`Self::is_pack_drop`] memoized for the length of one drag. Classifying
    /// a zip opens the archive and parses its whole central directory — tens of
    /// thousands of entries for a UAL Source zip — and the hover path asks
    /// twice per file per frame while the drag holds the UI repainting. Every
    /// drop path goes through here too, since the drop frame still has the
    /// verdicts the hover frames computed.
    fn is_pack_drop_cached(&mut self, path: &std::path::Path) -> bool {
        if let Some(&known) = self.pack_drop_cache.get(path) {
            return known;
        }
        let verdict = Self::is_pack_drop(path);
        self.pack_drop_cache.insert(path.to_path_buf(), verdict);
        verdict
    }

    /// A modal is up. Zones underneath it must not accept a drop: the user
    /// is answering a dialog, not importing, and the rect test alone cannot
    /// tell the two apart because the backdrop is painted over the zones,
    /// not through them.
    pub fn modal_open(&self) -> bool {
        let approval = {
            let state = self.state.lock().unwrap();
            state.awaiting_approval.is_some() || state.regenerating_image
        };
        approval
            || self.settings_modal.is_open
            || self.welcome_modal.is_open()
            || self.about_modal.is_open
            || self.show_template_editor
            || self.library_browser.is_open
            || self.show_demo_download_confirm
            || self.show_clip_packs_download_confirm
            || self.pending_delete_bundle.is_some()
            || self.pending_clear_animation
            || self.show_clear_history_confirmation
            || self.confirmation_dialog.is_open
    }

    /// Hovered OS-drag paths over `rect`, or nothing while a modal is open.
    /// Zones paint their hover overlay from this so the overlay and the drop
    /// agree on whether the zone is live.
    pub(crate) fn zone_hovered(&self, ctx: &egui::Context, rect: egui::Rect) -> Vec<PathBuf> {
        if self.modal_open() {
            return Vec::new();
        }
        hovered_paths_over(ctx, rect)
    }

    /// The shape every drop zone shares: paint a label while files hover,
    /// then on the drop frame claim exactly the files `accept` wants and
    /// return them. Rejected files stay in the frame's drop list for
    /// [`Self::drop_unclaimed`], which is the one place that says where a
    /// misdropped file belongs — a zone never toasts about what it declined.
    ///
    /// `accept` is `FnMut` so a zone that uses one file can stop after the
    /// first match instead of swallowing the batch.
    fn drop_zone(
        &mut self,
        ctx: &egui::Context,
        rect: egui::Rect,
        label: impl FnOnce(&mut Self, &[PathBuf]) -> Option<&'static str>,
        mut accept: impl FnMut(&mut Self, &std::path::Path) -> bool,
    ) -> Vec<PathBuf> {
        if self.modal_open() {
            return Vec::new();
        }
        let hovered = hovered_paths_over(ctx, rect);
        if !hovered.is_empty()
            && let Some(text) = label(self, &hovered)
        {
            paint_drop_overlay(ctx, rect, text);
        }
        let dropped = dropped_paths_over(ctx, rect);
        if dropped.is_empty() {
            return Vec::new();
        }
        let claimed: Vec<PathBuf> = dropped.into_iter().filter(|p| accept(self, p)).collect();
        claim_dropped(ctx, &claimed);
        claimed
    }

    /// Sidebar "Drop image here" — pipeline input, not a library bundle.
    /// Claims one still; anything else falls through to
    /// [`Self::drop_unclaimed`] and gets told where it belongs. The sidebar
    /// paints its own hover state, so no label here.
    pub(crate) fn drop_generation_image(&mut self, ctx: &egui::Context, rect: egui::Rect) {
        let mut taken = false;
        let claimed = self.drop_zone(
            ctx,
            rect,
            |_, _| None,
            |_, p| {
                let take = !taken && Self::is_image_file(p);
                taken |= take;
                take
            },
        );
        if let Some(path) = claimed.into_iter().next() {
            self.queue_image_for_generation(path.to_string_lossy().into_owned());
        }
    }

    /// Bundle Info pane — always a new library bundle. Takes everything, and
    /// [`Self::import_as_new_bundle`] says per file what could not be used.
    pub(crate) fn drop_import_on_bundle_info(&mut self, ctx: &egui::Context, rect: egui::Rect) {
        let dropped = self.drop_zone(
            ctx,
            rect,
            |app, hovered| {
                let saw_pack = hovered.iter().any(|p| app.is_pack_drop_cached(p));
                let saw_bundle = hovered
                    .iter()
                    .any(|p| Self::is_bundle_drop(p) && !app.is_pack_drop_cached(p));
                if saw_pack && !saw_bundle {
                    Some(clip_packs::DROP_PACKS_ON_ANIMATE)
                } else {
                    Some(clip_packs::DROP_IMPORT)
                }
            },
            |_, _| true,
        );
        if !dropped.is_empty() {
            self.import_as_new_bundle(dropped);
        }
    }

    /// Empty Image tab — attach `image.png` only. Claims one still.
    pub(crate) fn drop_attach_image(&mut self, ctx: &egui::Context, rect: egui::Rect) {
        let mut taken = false;
        let claimed = self.drop_zone(
            ctx,
            rect,
            |app, hovered| {
                if hovered.iter().any(|p| Self::is_image_file(p)) {
                    Some(clip_packs::DROP_ATTACH_IMAGE)
                } else if hovered.iter().any(|p| app.is_pack_drop_cached(p)) {
                    Some(clip_packs::DROP_PACKS_ON_ANIMATE)
                } else {
                    Some(clip_packs::DROP_BUNDLES_ON_INFO)
                }
            },
            |_, p| {
                let take = !taken && Self::is_image_file(p);
                taken |= take;
                take
            },
        );
        if let Some(path) = claimed.into_iter().next() {
            self.attach_to_current_bundle(path);
        }
    }

    /// Empty 3D tab — attach `model.glb` only. Claims one mesh that is not a
    /// clip pack.
    pub(crate) fn drop_attach_model(&mut self, ctx: &egui::Context, rect: egui::Rect) {
        let mut taken = false;
        let claimed = self.drop_zone(
            ctx,
            rect,
            |app, hovered| {
                if hovered
                    .iter()
                    .any(|p| Self::is_glb_file(p) && !app.is_pack_drop_cached(p))
                {
                    Some(clip_packs::DROP_ATTACH_MODEL)
                } else if hovered.iter().any(|p| app.is_pack_drop_cached(p)) {
                    Some(clip_packs::DROP_PACKS_ON_ANIMATE)
                } else {
                    Some(clip_packs::DROP_BUNDLES_ON_INFO)
                }
            },
            |app, p| {
                let take = !taken && Self::is_glb_file(p) && !app.is_pack_drop_cached(p);
                taken |= take;
                take
            },
        );
        if let Some(path) = claimed.into_iter().next() {
            self.attach_to_current_bundle(path);
        }
    }

    /// Animation panel — clip packs only.
    pub(crate) fn drop_install_pack(&mut self, ctx: &egui::Context, rect: egui::Rect) {
        let packs = self.drop_zone(
            ctx,
            rect,
            |app, hovered| {
                if hovered.iter().any(|p| app.is_pack_drop_cached(p)) {
                    Some(clip_packs::DROP_INSTALL_PACK)
                } else {
                    // Not "drop packs on the Animation panel" — they are
                    // already on it. Name where a mesh or still actually goes.
                    Some(clip_packs::DROP_BUNDLES_ON_INFO)
                }
            },
            |app, p| app.is_pack_drop_cached(p),
        );
        for path in packs {
            self.install_clip_pack(path);
        }
    }

    /// Runs at the end of [`eframe::App::ui`], after every zone has had its
    /// chance. A drop must never vanish, and there are three ways it can
    /// reach here:
    ///
    /// - **A modal is open.** Every zone declined on purpose. Say so, rather
    ///   than importing behind a dialog the user is still answering.
    /// - **No pointer at all.** No zone could match, because none of them can
    ///   know where the drop landed: the platform can't report the cursor
    ///   during a drag (X11 has no drag coordinates, Wayland no file drops),
    ///   or [`crate::dnd`]'s query failed this frame.
    ///   Route by file type, which is what shipped before zones existed.
    /// - **Dropped outside every zone** — the menu bar, the progress panel, or
    ///   a zone that had no use for this file type. Say where it belongs
    ///   rather than guessing, since guessing "new bundle" for a window-wide
    ///   drop is the behavior that made the zones necessary.
    pub(crate) fn drop_unclaimed(&mut self, ctx: &egui::Context) {
        let dropped: Vec<std::path::PathBuf> = ctx.input_mut(|i| {
            std::mem::take(&mut i.raw.dropped_files)
                .into_iter()
                .filter_map(|f| f.path)
                .collect()
        });
        if dropped.is_empty() {
            return;
        }

        if self.modal_open() {
            self.add_toast(Toast::info("Close the dialog first, then drop again"));
            return;
        }

        if ctx.pointer_latest_pos().is_some() {
            // Zones were live and none of them wanted it.
            if dropped.iter().all(|p| self.is_pack_drop_cached(p)) {
                self.add_toast(Toast::info(clip_packs::DROP_PACKS_ON_ANIMATE));
            } else {
                self.add_toast(Toast::info(clip_packs::DROP_BUNDLES_ON_INFO));
            }
            return;
        }

        let packs: Vec<_> = dropped
            .iter()
            .filter(|p| self.is_pack_drop_cached(p))
            .cloned()
            .collect();
        let route = route_without_zones(dropped, &packs);
        for path in route.packs {
            self.install_clip_pack(path);
        }
        if let Some(path) = route.generation_image {
            self.queue_image_for_generation(path.to_string_lossy().into_owned());
        }
        if !route.new_bundle.is_empty() {
            self.import_as_new_bundle(route.new_bundle);
        }
    }

    /// Bundle Info import: always a new bundle. Packs are not wrapped; every
    /// other file is handed to core, whose refusal (a `.gltf` without its
    /// sidecar buffers, an unknown type) comes back as a toast naming the
    /// file, so nothing dropped here is skipped without a word.
    fn import_as_new_bundle(&mut self, dropped: Vec<std::path::PathBuf>) {
        let mut loose_glbs = Vec::new();
        let mut loose_images = Vec::new();
        for path in dropped {
            if self.is_pack_drop_cached(&path) {
                self.add_toast(Toast::info(format!(
                    "{}: {}",
                    file_label(&path),
                    clip_packs::DROP_PACKS_ON_ANIMATE
                )));
            } else if Self::is_glb_file(&path) {
                loose_glbs.push(path);
            } else if Self::is_image_file(&path) {
                loose_images.push(path);
            } else {
                self.import_bundle(path);
            }
        }
        for files in plan_loose_imports(loose_glbs, loose_images) {
            self.import_loose_files(files);
        }
    }

    /// Import a bundle zip/folder, a bundle.json inside one, or a loose GLB
    /// / image. Pickers can't make "double-click a folder" mean SELECT on
    /// macOS (it navigates), but double-clicking the folder's bundle.json
    /// is unambiguous. A lone GLB or image is wrapped into a library bundle.
    fn import_bundle(&mut self, source: std::path::PathBuf) {
        // Normalize bundle.json → its containing directory. Any OTHER .json
        // is a wrong pick — say so instead of a misleading archive error.
        let is_metadata = source
            .file_name()
            .is_some_and(|n| n == asset_tap_core::constants::files::bundle::METADATA);
        if !is_metadata
            && source
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("json"))
        {
            self.add_toast(Toast::error(
                "That JSON isn't a bundle.json. Pick the bundle.json inside a bundle folder, a .zip, a .glb, or an image",
            ));
            return;
        }
        let source = if is_metadata {
            match source.parent() {
                Some(dir) => dir.to_path_buf(),
                None => source,
            }
        } else {
            source
        };
        self.queued_imports.push_back(QueuedImport::Bundle(source));
        self.pump_imports();
    }

    fn import_loose_files(&mut self, sources: Vec<std::path::PathBuf>) {
        if sources.is_empty() {
            return;
        }
        if sources.len() == 1 {
            self.import_bundle(sources.into_iter().next().unwrap());
            return;
        }
        self.queued_imports.push_back(QueuedImport::Loose(sources));
        self.pump_imports();
    }

    /// Start the next queued import if none is in flight. Called after every
    /// enqueue and again when one completes, so a multi-file drop runs them
    /// one at a time instead of losing all but the last channel.
    fn pump_imports(&mut self) {
        if self.pending_import.is_some() {
            return;
        }
        let Some(job) = self.queued_imports.pop_front() else {
            return;
        };
        let output_dir = self.settings.output_dir.clone();
        // Several files can be queued from one drop, so a refusal has to say
        // which one it is about.
        let label = match &job {
            QueuedImport::Bundle(source) => file_label(source),
            QueuedImport::Loose(sources) => sources
                .iter()
                .map(|s| file_label(s))
                .collect::<Vec<_>>()
                .join(" + "),
        };
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending_import = Some(rx);
        self.add_toast(Toast::info(clip_packs::IMPORTING));
        self.runtime.spawn(async move {
            let result = tokio::task::spawn_blocking(move || match job {
                QueuedImport::Bundle(source) => asset_tap_core::import_bundle(&source, &output_dir),
                QueuedImport::Loose(sources) => {
                    asset_tap_core::import_loose_files(&sources, &output_dir)
                }
            })
            .await
            .unwrap_or_else(|e| Err(format!("Import task failed: {}", e)))
            .map_err(|e| format!("{label}: {e}"));
            let _ = tx.send(result);
        });
    }

    pub(crate) fn attach_to_current_bundle(&mut self, source: std::path::PathBuf) {
        let Some(bundle_dir) = self.output.as_ref().and_then(|o| o.output_dir.clone()) else {
            self.add_toast(Toast::error("Open a bundle first"));
            return;
        };
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending_attach = Some(rx);
        self.add_toast(Toast::info(clip_packs::ATTACHING));
        self.runtime.spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                asset_tap_core::attach_to_bundle(&bundle_dir, &source)
            })
            .await
            .unwrap_or_else(|e| Err(format!("Attach task failed: {e}")));
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
        let nothing_to_do = is_no_op_run(self.skip_3d, self.existing_image.is_some());

        !running
            && !nothing_to_do
            && (!self.prompt.is_empty() || self.existing_image.is_some())
            && self.effective_prompt_len()
                <= asset_tap_core::constants::validation::MAX_PROMPT_LENGTH
            && self.settings.has_required_api_keys(&self.provider_registry)
            && has_image_model
            && (self.skip_3d || (!self.model_3d_provider.is_empty() && !self.model_3d.is_empty()))
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
        if is_no_op_run(self.skip_3d, self.existing_image.is_some()) {
            return Some(
                "Nothing to generate: the input image replaces image generation, and \
                 image-only skips 3D. Clear one of them."
                    .to_string(),
            );
        }
        if !self.skip_3d && (self.model_3d_provider.is_empty() || self.model_3d.is_empty()) {
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

        // A picked file can be moved or deleted before Generate is pressed.
        // Without this the path falls through to the pipeline's remote-URL
        // branch and fails as a download error, which says nothing about the
        // real problem. The CLI rejects the same case up front.
        if let Some(ref image) = self.existing_image
            && !is_remote_url(image)
            && !std::path::Path::new(image).exists()
        {
            let name = std::path::Path::new(image)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| image.clone());
            self.toasts.push(Toast::error(format!(
                "Input image no longer exists: {name}. Choose another image."
            )));
            self.existing_image = None;
            return;
        }

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

        // Honored as selected. `can_generate` blocks the one combination that
        // would produce nothing (image-only plus an input image), so there is
        // no contradictory state left to reconcile here.
        if self.skip_3d {
            config = config.with_skip_3d();
        }

        // Enable approval if required by settings (and image is being
        // generated, not using existing, and we're actually going to 3D —
        // the approval gate asks "continue to 3D?", which is meaningless
        // when the pipeline stops after the image stage).
        if self.settings.require_image_approval && self.existing_image.is_none() && !self.skip_3d {
            config = config.with_image_approval();
        }

        // Shared cell the pipeline fills with its own generation directory as
        // soon as it's created. Used by failure-recovery below to locate this
        // run's saved image reliably, instead of guessing by sorting the
        // output directory (which can pick the wrong bundle).
        let gen_dir_cell: std::sync::Arc<std::sync::Mutex<Option<std::path::PathBuf>>> =
            std::sync::Arc::new(std::sync::Mutex::new(None));
        config.gen_dir_out = Some(gen_dir_cell.clone());

        // Start tracking in history
        let generation_id = {
            let mut history = self.history.lock().unwrap();
            history.start_generation(&config)
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
        let gen_dir_cell = gen_dir_cell.clone();

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
                        // Locate this run's saved image. Prefer the exact
                        // generation directory the pipeline reported via the
                        // shared cell (reliable). Fall back to a robust scan of
                        // the output directory that only considers
                        // timestamp-format dir names and picks the newest by
                        // parsed timestamp — never a plain lexicographic guess.
                        let image_path = gen_dir_cell
                            .lock()
                            .unwrap()
                            .clone()
                            .map(|dir| dir.join(bundle_files::IMAGE))
                            .filter(|p| p.exists())
                            .or_else(|| latest_run_image(&output_dir));

                        if let Some(image_path) = image_path {
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
                .add_filter("Images", asset_tap_core::constants::files::IMAGE_EXTS)
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
        if Self::is_image_file(std::path::Path::new(&path)) {
            self.existing_image = Some(path);
            return true;
        }
        false
    }

    /// Queue an image as the pipeline's `existing_image` and notify the user
    /// with a success toast. Returns whether the path was accepted. All UI
    /// entry points that let the user pick an image for the next generation
    /// (library context menu, preview-pane button/menu, sidebar library
    /// selection) go through this so the toast copy and validation stay
    /// consistent.
    pub fn queue_image_for_generation(&mut self, path: String) -> bool {
        let accepted = self.set_existing_image(path);
        if accepted {
            self.add_toast(Toast::success("Image queued for generation"));
        }
        accepted
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
        self.approval_texture_failed = None;
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
        self.approval_texture_failed = None;
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
        self.approval_texture_failed = None;

        // Clear the approval data (modal will show loading state)
        state.awaiting_approval = None;
        state.regenerating_image = true;

        // Send regenerate signal to pipeline (it will re-run image generation and send new AwaitingApproval)
        if let Some(ref tx) = state.approval_tx {
            let _ = tx.send(ApprovalResponse::Regenerate);
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
                if let Some(path) = paths.first() {
                    self.queue_image_for_generation(path.to_string_lossy().into_owned());
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
                        self.reset_animate_for_new_asset();
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
        if self.output_swap_blocked() {
            return;
        }
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
        self.reset_animate_for_new_asset();

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
        if self.output_swap_blocked() {
            // Still make it findable: the dropdown is where the user will
            // look for it once the run is over.
            self.bundle_info_panel
                .refresh_bundle_list(&self.settings.output_dir);
            return false;
        }
        match load_bundle(&bundle_dir) {
            Ok(bundle) => {
                let output = PipelineOutput::from(bundle);
                if let Some(tab) = pick_preview_tab_for_output(&output) {
                    self.preview_tab = tab;
                }
                self.output = Some(output);
                self.reset_animate_for_new_asset();
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

    /// Remove a bundle directory after the user confirmed, and drop it from
    /// the view if it was the one open.
    fn delete_bundle(&mut self, path: &std::path::Path) {
        match std::fs::remove_dir_all(path) {
            Ok(()) => {
                self.add_toast(Toast::success("Bundle deleted"));
                if self
                    .app_state
                    .current_generation
                    .as_deref()
                    .is_some_and(|p| p == path)
                {
                    self.output = None;
                    self.app_state.current_generation = None;
                    self.bundle_info_panel.current_bundle = None;
                    self.reset_animate_for_new_asset();
                }
                self.bundle_info_panel
                    .refresh_bundle_list(&self.settings.output_dir);
            }
            Err(e) => {
                tracing::error!("Failed to delete bundle: {}", e);
                self.toasts
                    .push(Toast::error(format!("Failed to delete: {e}")));
            }
        }
    }

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
            match self.app_state.save() {
                Ok(_) => self.toasts.push(Toast::success("Prompt history cleared")),
                Err(e) => {
                    tracing::error!("Failed to save state after clearing history: {}", e);
                    self.toasts
                        .push(Toast::error("Failed to clear prompt history"));
                }
            }
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

    pub fn workbench_busy(&self) -> bool {
        self.pending_workbench.is_some()
    }

    /// Run `work` off-thread against `model`, tagged with a fresh request id.
    /// One job at a time; returns `false` if one is already in flight. The
    /// completion is applied only if [`workbench_completion_is_current`]
    /// still holds when it lands.
    fn start_workbench_job(
        &mut self,
        model: PathBuf,
        work: impl FnOnce(&std::path::Path) -> Result<WorkbenchDone, String> + Send + 'static,
    ) -> bool {
        if self.pending_workbench.is_some() {
            return false;
        }
        self.workbench_seq += 1;
        let job = WorkbenchJob {
            id: self.workbench_seq,
            model: model.clone(),
        };
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending_workbench = Some((job, rx));
        self.runtime.spawn(async move {
            let result = tokio::task::spawn_blocking(move || work(&model))
                .await
                .map_err(|e| e.to_string())
                .and_then(std::convert::identity);
            let _ = tx.send(result);
        });
        true
    }

    /// The model on screen is changing. Any job in flight was issued for the
    /// old one; bumping the sequence makes its completion stale.
    fn invalidate_workbench(&mut self) {
        self.workbench_seq += 1;
    }

    /// Inspect is the default. Closing the Animation panel stops playback
    /// and hides bones; it does not write the asset.
    pub fn close_animate_panel(&mut self) {
        self.workbench_animate = false;
        let mut viewer = self.model_viewer.lock().unwrap();
        self.workbench_show_bones = viewer.show_bones;
        viewer.set_playing(false);
        viewer.set_show_bones(false);
    }

    pub fn open_animate_panel(&mut self) {
        if self.pending_workbench.is_some() {
            self.toasts
                .push(Toast::info("Wait for the current job to finish"));
            return;
        }
        let Some(model) = self.current_model_path() else {
            self.toasts.push(Toast::error(
                "No model loaded. Generate or open a bundle first",
            ));
            return;
        };
        if self.model_viewer.lock().unwrap().is_loading() {
            self.toasts.push(Toast::info(
                "The model is still loading. Try again in a moment",
            ));
            return;
        }
        self.workbench_animate = true;
        self.refresh_clip_catalog();
        // The file is read once, off-thread, in the `Opened` job below. Until
        // it lands the panel shows nothing ticked rather than the previous
        // model's set.
        self.bake_set.clear();
        self.model_clips.clear();
        let mut viewer = self.model_viewer.lock().unwrap();
        viewer.set_show_bones(self.workbench_show_bones);
        // Unfitted: land in Rig. Playback is locked until Bind.
        // Fitted (prior Bind / CLI --rig): stay on the clip viewer.
        let entered = (!viewer.is_fitted()).then(|| viewer.enter_place());
        let needs_seed = viewer.awaiting_seed();
        drop(viewer);
        if let Some(Err(e)) = entered {
            self.toasts.push(Toast::error(e));
            return;
        }
        // Rig opens with the shipped skeleton scaled to the mesh, never with an
        // auto-fit. Solving on open meant the first thing anyone saw was either
        // magic or nonsense, and when the solve failed there were no joints at
        // all and no way to summon any: a monitor left the panel with an empty
        // skeleton and a greyed-out Bind. Placing the skeleton is the user's
        // job; Auto-fit is a button they reach for once they are in here.
        //
        // The same trip reads the baked set and, for an unfitted mesh, whether
        // a skin we cannot name is about to be replaced: that is the right
        // default and a poor surprise, so say it before the author spends time
        // arranging joints. None of this reads the file on the UI thread.
        self.start_workbench_job(model, move |path| {
            let clips = asset_tap_core::baked_clip_names(path).unwrap_or_default();
            let (foreign_joints, markers) = if needs_seed {
                let foreign = asset_tap_core::foreign_rig_joints(path).ok().flatten();
                let markers =
                    asset_tap_core::default_bind_markers(path).map_err(|e| e.to_string())?;
                if markers.is_empty() {
                    return Err("That produced no joints to place".to_string());
                }
                (foreign, Some(markers))
            } else {
                (None, None)
            };
            Ok(WorkbenchDone::Opened {
                clips,
                foreign_joints,
                markers,
            })
        });
    }

    fn reset_animate_for_new_asset(&mut self) {
        self.invalidate_workbench();
        self.model_viewer.lock().unwrap().exit_place();
        self.close_animate_panel();
        self.bake_set.clear();
        self.model_clips.clear();
    }

    #[allow(dead_code)]
    pub fn preview_obscured(&self) -> bool {
        self.modal_open() || self.preview_tab != PreviewTab::Model3D
    }

    pub fn preview_clip(&mut self, id: &str) {
        if self.refuse_if_placing() {
            return;
        }
        let Some(model) = self.current_model_path() else {
            self.toasts.push(Toast::error(
                "No model loaded. Generate or open a bundle first",
            ));
            return;
        };
        let clip_id = id.to_string();
        self.start_workbench_job(model, move |path| {
            asset_tap_core::preview_skinned_clip(&clip_id, path)
                .map(|clip| WorkbenchDone::Preview {
                    clip: Box::new(clip),
                })
                .map_err(|e| e.to_string())
        });
    }

    /// Auto-fit in the Rig step: solve the skeleton onto the mesh's landmarks
    /// off-thread, then drop it into the open Rig session for review. Never
    /// writes. Goes through the workbench channel so the panel shows busy
    /// while the mesh is re-read.
    pub fn start_reseed(&mut self) {
        if !self.model_viewer.lock().unwrap().is_placing() {
            return;
        }
        let Some(model) = self.current_model_path() else {
            self.toasts.push(Toast::error("No model loaded"));
            return;
        };
        self.start_workbench_job(model, |path| {
            let markers = asset_tap_core::seed_bind_markers(path).map_err(|e| e.to_string())?;
            if markers.is_empty() {
                return Err("That produced no joints to place".to_string());
            }
            Ok(WorkbenchDone::Seed { markers })
        });
    }

    pub fn start_fit(&mut self, model: PathBuf, heads: Option<Vec<(String, [f32; 3])>>) {
        // Bind rewrites the skin; the clips already baked in are re-sourced
        // onto the new rig rather than lost with it.
        let keep: Vec<String> = self.model_clips.iter().cloned().collect();
        self.start_workbench_job(model, move |path| {
            let options = asset_tap_core::BindOptions {
                fit_only: true,
                ..Default::default()
            };
            let report = match heads.as_deref() {
                Some(heads) if !heads.is_empty() => {
                    asset_tap_core::rig::fit_mesh_from_heads_keeping(
                        path, path, heads, &keep, &options,
                    )
                    .map_err(|e| e.to_string())?
                }
                _ => asset_tap_core::fit_mesh(path, path, &options).map_err(|e| e.to_string())?,
            };
            let clips = report.clips.clone();
            let dropped_clips = report.dropped_clips.clone();
            if let Some(dir) = path.parent() {
                asset_tap_core::stamp_bind_step(dir, &clips).map_err(|e| e.to_string())?;
            }
            Ok(WorkbenchDone::Fit {
                moved_joints: report.moved_joints,
                max_moved_m: report.max_moved_m,
                clips,
                dropped_clips,
            })
        });
    }

    pub fn start_bake(&mut self, model: PathBuf, clips: Vec<String>) {
        if self.refuse_if_placing() {
            return;
        }
        self.start_workbench_job(model, move |path| {
            let options = asset_tap_core::BindOptions {
                clips: clips.clone(),
                fit_only: false,
                ..Default::default()
            };
            asset_tap_core::apply_clip(path, path, &options).map_err(|e| e.to_string())?;
            if let Some(dir) = path.parent() {
                asset_tap_core::stamp_bind_step(dir, &clips).map_err(|e| e.to_string())?;
            }
            Ok(WorkbenchDone::Bake { clips })
        });
    }

    /// Record what the file holds now and reset the ticked set to match.
    ///
    /// Bake is declarative, so this is the starting point the author edits:
    /// tick to add, untick to remove, Bake writes exactly what is ticked.
    /// Fed by completions, never by a read on the UI thread.
    fn set_model_clips(&mut self, clips: Vec<String>) {
        self.model_clips = clips.into_iter().collect();
        self.bake_set.clone_from(&self.model_clips);
    }

    /// Ticked clips Bake can write, and the ticked names no installed pack
    /// supplies. See [`split_bake_set`].
    pub fn bake_plan(&self) -> (Vec<String>, Vec<String>) {
        split_bake_set(
            &self.bake_set,
            self.clip_catalog.iter().map(|c| c.id.as_str()),
        )
    }

    /// Clips a Bake would add and remove, given what is on disk. Counts only
    /// what Bake can write: an unavailable name in the ticked set is a
    /// removal, and the panel says so.
    pub fn bake_delta(&self) -> (usize, usize) {
        let (available, _) = self.bake_plan();
        bake_delta_of(&available.into_iter().collect(), &self.model_clips)
    }

    /// Re-read the merged clip catalog and keep the selection valid.
    ///
    /// Every listed clip is installed by definition, so a selection that is no
    /// longer present means its pack was removed — fall back to the first clip
    /// rather than leaving a name nothing can resolve.
    pub fn refresh_clip_catalog(&mut self) {
        self.clip_catalog = asset_tap_core::list_clips();
        if self.clip_catalog.iter().any(|c| c.id == self.clip) {
            return;
        }
        let resolved = asset_tap_core::find_clip(&self.clip)
            .ok()
            .map(|(_, name)| name)
            .or_else(|| self.clip_catalog.first().map(|c| c.id.clone()));
        if let Some(id) = resolved {
            self.clip = id;
        }
    }

    pub fn install_clip_pack(&mut self, source: std::path::PathBuf) {
        // pack.json → its containing directory, same as bundle.json → folder.
        let is_manifest = source
            .file_name()
            .is_some_and(|n| n == asset_tap_core::PACK_MANIFEST);
        let source = if is_manifest {
            match source.parent() {
                Some(dir) => dir.to_path_buf(),
                None => source,
            }
        } else {
            source
        };
        self.queued_pack_installs.push_back(source);
        self.pump_pack_installs();
    }

    /// Start the next queued install once nothing else is touching
    /// `packs_root()`. Installs wait on each other and on the workbench: two
    /// at once would lose a completion channel, and one racing a bind/bake
    /// would rewrite a pack the workbench is reading.
    fn pump_pack_installs(&mut self) {
        if self.pending_pack_install.is_some() || self.pending_workbench.is_some() {
            return;
        }
        let Some(source) = self.queued_pack_installs.pop_front() else {
            return;
        };
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending_pack_install = Some(rx);
        self.add_toast(Toast::info(clip_packs::INSTALLING_PACK));
        self.runtime.spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                asset_tap_core::install_pack_from(&source, None)
                    .map(|p| (p.name, p.id, p.clips.len()))
                    .map_err(|e| e.to_string())
            })
            .await
            .unwrap_or_else(|e| Err(format!("Install task failed: {e}")));
            let _ = tx.send(result);
        });
    }

    fn poll_workbench(&mut self, ctx: &egui::Context) {
        let Some((job, rx)) = self.pending_workbench.take() else {
            return;
        };
        let mut slot = Some(rx);
        let Some(result) = poll_oneshot(&mut slot, ctx, "Workbench") else {
            if let Some(rx) = slot {
                self.pending_workbench = Some((job, rx));
            }
            return;
        };
        let current = self.current_model_path();
        if !workbench_completion_is_current(&job, current.as_deref(), self.workbench_seq) {
            // The user moved on while this ran. Applying it now would reload
            // or re-tick a model it was never about.
            tracing::debug!(
                job = job.id,
                latest = self.workbench_seq,
                model = %job.model.display(),
                "dropping stale workbench completion"
            );
            return;
        }
        match result {
            Ok(done) => match done {
                WorkbenchDone::Fit {
                    moved_joints,
                    max_moved_m,
                    clips,
                    dropped_clips,
                } => {
                    // Say what the bind consumed, so "did my pose get in?"
                    // is answered on screen rather than inferred from the walk.
                    let msg = match moved_joints {
                        0 => "Rig bound to the auto-fit pose".to_string(),
                        1 => {
                            format!("Rig bound, 1 joint moved from auto-fit ({max_moved_m:.2} m)")
                        }
                        n => format!(
                            "Rig bound, {n} joints moved from auto-fit (max {max_moved_m:.2} m)"
                        ),
                    };
                    self.toasts.push(Toast::success(msg));
                    if !dropped_clips.is_empty() {
                        self.toasts.push(Toast::info(format!(
                            "Re-bind could not keep {}: {}. Bake again to restore",
                            match dropped_clips.len() {
                                1 => "1 baked clip".to_string(),
                                n => format!("{n} baked clips"),
                            },
                            dropped_clips.join(", ")
                        )));
                    }
                    let mut viewer = self.model_viewer.lock().unwrap();
                    viewer.exit_place();
                    viewer.reload();
                    drop(viewer);
                    // Bind lands on the clip list with nothing playing. The
                    // "N joints moved" toast is the confirmation the pose went
                    // in; a walk cycle starting on its own is not.
                    self.set_model_clips(clips);
                }
                WorkbenchDone::Bake { clips } => {
                    let msg = match clips.len() {
                        0 => "Cleared animation from model.glb".to_string(),
                        1 => "Baked 1 clip into model.glb".to_string(),
                        n => format!("Baked {n} clips into model.glb"),
                    };
                    self.toasts.push(Toast::success(msg));
                    self.model_viewer.lock().unwrap().reload();
                    self.set_model_clips(clips);
                }
                WorkbenchDone::Seed { markers } => {
                    self.model_viewer
                        .lock()
                        .unwrap()
                        .apply_seeded_markers(markers);
                }
                WorkbenchDone::Preview { clip } => {
                    self.model_viewer.lock().unwrap().set_clip(*clip);
                }
                WorkbenchDone::Opened {
                    clips,
                    foreign_joints,
                    markers,
                } => {
                    self.set_model_clips(clips);
                    if let Some(markers) = markers {
                        // No-op if the user already left Rig.
                        self.model_viewer
                            .lock()
                            .unwrap()
                            .apply_seeded_markers(markers);
                    }
                    if let Some(n) = foreign_joints {
                        self.toasts.push(Toast::info(format!(
                            "This model already has a rig of {n} joints that Asset Tap cannot read. \
                             Bind will replace it."
                        )));
                    }
                }
            },
            Err(e) => {
                // Auto-fit failing is soft: Rig already holds the default
                // skeleton, so the author simply keeps the joints they have and
                // places them by hand. Only a session that never got a skeleton
                // at all backs out, which now means the mesh had no geometry.
                let mut viewer = self.model_viewer.lock().unwrap();
                if viewer.awaiting_seed() {
                    viewer.exit_place();
                }
                drop(viewer);
                self.toasts.push(Toast::error(e));
            }
        }
    }

    fn refuse_if_placing(&mut self) -> bool {
        let placing = self.model_viewer.lock().unwrap().is_placing();
        if placing {
            self.toasts.push(Toast::error(
                "Bind or Cancel the rig before Preview or Bake",
            ));
        }
        placing
    }

    pub fn commit_place(&mut self) {
        if self.pending_workbench.is_some() {
            return;
        }
        let heads = self.model_viewer.lock().unwrap().place_world_heads();
        if heads.is_empty() {
            self.toasts
                .push(Toast::error("Nothing to bind. Pose the skeleton first"));
            return;
        }
        let Some(path) = self.current_model_path() else {
            self.toasts.push(Toast::error("No model loaded"));
            return;
        };
        self.start_fit(path, Some(heads));
    }

    fn current_model_path(&self) -> Option<PathBuf> {
        self.output
            .as_ref()
            .and_then(|o| o.final_model_path().map(|p| p.to_path_buf()))
    }
}

/// Where a drop goes when there are no zones to land in.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ZonelessRoute {
    pub packs: Vec<PathBuf>,
    /// A lone still queues for generation, as it did before zones existed —
    /// on a zoneless platform the sidebar slot is otherwise unreachable.
    pub generation_image: Option<PathBuf>,
    pub new_bundle: Vec<PathBuf>,
}

/// Split a drop by file type, for the platforms that can't report where it
/// landed. `packs` is the subset already classified as clip packs, passed in
/// because classifying a zip is expensive enough to be memoized by the caller.
pub(crate) fn route_without_zones(dropped: Vec<PathBuf>, packs: &[PathBuf]) -> ZonelessRoute {
    let mut route = ZonelessRoute::default();
    let (stills, assets): (Vec<_>, Vec<_>) = dropped
        .into_iter()
        .filter(|p| {
            if packs.contains(p) {
                route.packs.push(p.clone());
                false
            } else {
                true
            }
        })
        .partition(|p| App::is_image_file(p));

    if assets.is_empty() {
        // Several stills and no mesh: the first is the generation input, the
        // rest have nowhere sensible to go without a zone.
        route.generation_image = stills.into_iter().next();
    } else {
        route.new_bundle = assets;
        route.new_bundle.extend(stills);
    }
    route
}

impl eframe::App for App {
    /// winit gives a file drag no coordinates, so nothing in egui knows where
    /// a drop landed. Ask the OS and inject it as a pointer move before the
    /// frame runs; every drop zone then works off ordinary egui hover state.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if raw_input.hovered_files.is_empty() && raw_input.dropped_files.is_empty() {
            self.pack_drop_cache.clear();
        }
        crate::dnd::inject_drag_pointer(ctx, raw_input, &mut self.drag_pointer);
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_workbench(ctx);
        // A queued install may have been waiting on that workbench job.
        self.pump_pack_installs();
        let dt = ctx.input(|i| i.stable_dt);
        {
            let mut viewer = self.model_viewer.lock().unwrap();
            viewer.tick(dt);
            if viewer.is_playing() {
                ctx.request_repaint();
            }
        }

        // Captured under the state lock and processed after release; see the
        // error-toast branch below for context.
        let mut pending_recovery_bundle: Option<PathBuf> = None;
        // Likewise: the reset locks the viewer and bumps the workbench
        // sequence, which needs `&mut self` after the state guard is gone.
        let mut swapped_output = false;

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
                swapped_output = true;

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
        if swapped_output {
            self.reset_animate_for_new_asset();
        }
        if let Some(bundle_dir) = pending_recovery_bundle {
            self.activate_bundle_from_dir(bundle_dir);
        }

        // Completed file selection; `None` is a cancelled dialog.
        if let Some(Some(path)) = poll_oneshot(&mut self.pending_file_selection, ctx, "File dialog")
        {
            self.existing_image = Some(path.to_string_lossy().to_string());
        }

        match poll_oneshot(&mut self.pending_export, ctx, "Export") {
            Some(Ok(msg)) => self.add_toast(Toast::success(msg)),
            Some(Err(msg)) => self.toasts.push(Toast::error(msg)),
            None => {}
        }

        match poll_oneshot(&mut self.pending_import, ctx, "Import") {
            Some(Ok(bundle_dir)) => {
                self.add_toast(Toast::success("Bundle imported"));
                self.activate_bundle_from_dir(bundle_dir);
            }
            Some(Err(msg)) => {
                tracing::error!("Bundle import failed: {}", msg);
                self.toasts
                    .push(Toast::error(format!("Import failed: {msg}")));
            }
            None => {}
        }
        self.pump_imports();

        match poll_oneshot(&mut self.pending_attach, ctx, "Attach") {
            Some(Ok(bundle_dir)) => {
                let tab = self.preview_tab;
                self.add_toast(Toast::success("Added to bundle"));
                self.activate_bundle_from_dir(bundle_dir);
                self.preview_tab = tab;
            }
            Some(Err(msg)) => {
                tracing::error!("Bundle attach failed: {}", msg);
                self.toasts
                    .push(Toast::error(format!("Could not add to bundle: {msg}")));
            }
            None => {}
        }

        match poll_oneshot(&mut self.pending_pack_install, ctx, "Pack install") {
            Some(Ok((name, id, clips))) => {
                self.refresh_clip_catalog();
                self.add_toast(Toast::success(format!(
                    "Installed {name} ({id}) with {clips} clips"
                )));
            }
            Some(Err(msg)) => {
                tracing::error!("Clip pack install failed: {}", msg);
                self.toasts
                    .push(Toast::error(format!("Pack install failed: {msg}")));
            }
            None => {}
        }
        self.pump_pack_installs();

        match poll_oneshot(&mut self.pending_demo_download, ctx, "Demo download") {
            Some(Ok(asset_tap_core::DemoDownloadResult::Downloaded(demo_dir))) => {
                self.add_toast(Toast::success("Demo assets downloaded"));
                self.activate_bundle_from_dir(demo_dir);
            }
            Some(Ok(asset_tap_core::DemoDownloadResult::AlreadyExists(v))) => {
                self.toasts
                    .push(Toast::info(format!("Demo bundle v{v} already downloaded")));
            }
            Some(Err(msg)) => {
                tracing::error!("Demo bundle download failed: {}", msg);
                self.toasts
                    .push(Toast::error("Failed to download demo assets"));
            }
            None => {}
        }

        match poll_oneshot(
            &mut self.pending_clip_packs_download,
            ctx,
            "Clip-pack download",
        ) {
            Some(Ok(asset_tap_core::ClipPacksDownloadResult::Downloaded { installed, .. })) => {
                self.refresh_clip_catalog();
                self.toasts.push(Toast::success(format!(
                    "Installed Universal Animation Libraries ({})",
                    installed.join(", "),
                )));
            }
            Some(Ok(asset_tap_core::ClipPacksDownloadResult::AlreadyExists { .. })) => {
                self.toasts.push(Toast::info(
                    "Universal Animation Libraries already installed",
                ));
            }
            Some(Err(msg)) => {
                tracing::error!("Clip-pack download failed: {}", msg);
                self.toasts.push(Toast::error(
                    "Failed to download Universal Animation Libraries",
                ));
            }
            None => {}
        }

        // Update library browser with current output directory
        self.library_browser
            .set_output_dir(self.settings.output_dir.clone());
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Detach the Context from `ui`'s borrow so modals/windows can take it
        // while panels borrow `ui` mutably.
        let ctx = &ui.ctx().clone();

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
                    self.toasts.push(Toast::error("Failed to save settings"));
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

        use views::confirmation_dialog::{ConfirmOutcome, ConfirmSpec, render_confirm};

        // Demo download confirmation dialog. Expensive (34 MB), so no
        // Enter-to-confirm; `render_confirm` never binds one.
        if self.show_demo_download_confirm {
            match render_confirm(
                ctx,
                &ConfirmSpec {
                    id: "demo_download_confirm_backdrop",
                    title: "Download Demo Bundle",
                    message: "Download a sample asset bundle with a generated Image and 3D Model?"
                        .to_string(),
                    detail: Some(format!(
                        "This will download approximately {DEMO_BUNDLE_SIZE_LABEL}."
                    )),
                    destructive: false,
                    confirm_label: format!("{} Download", crate::icons::DOWNLOAD),
                },
            ) {
                ConfirmOutcome::Confirmed => {
                    self.show_demo_download_confirm = false;
                    self.start_demo_download();
                }
                ConfirmOutcome::Dismissed => self.show_demo_download_confirm = false,
                ConfirmOutcome::Pending => {}
            }
        }

        if self.show_clip_packs_download_confirm {
            match render_confirm(
                ctx,
                &ConfirmSpec {
                    id: "clip_packs_download_confirm_backdrop",
                    title: clip_packs::DOWNLOAD_ACTION,
                    message: clip_packs::DOWNLOAD_PROMPT.to_string(),
                    detail: Some(clip_packs::download_detail()),
                    destructive: false,
                    confirm_label: format!("{} Download", crate::icons::DOWNLOAD),
                },
            ) {
                ConfirmOutcome::Confirmed => {
                    self.show_clip_packs_download_confirm = false;
                    self.start_clip_packs_download();
                }
                ConfirmOutcome::Dismissed => self.show_clip_packs_download_confirm = false,
                ConfirmOutcome::Pending => {}
            }
        }

        // Delete bundle confirmation dialog. Destructive: explicit click only.
        if let Some(bundle_path) = self.pending_delete_bundle.clone() {
            let bundle_name = bundle_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("this bundle");
            match render_confirm(
                ctx,
                &ConfirmSpec {
                    id: "delete_bundle_confirm_backdrop",
                    title: "Delete Bundle",
                    message: format!("Permanently delete \"{bundle_name}\"?"),
                    detail: Some(
                        "This will delete the bundle directory and all its contents. \
                         This action cannot be undone."
                            .to_string(),
                    ),
                    destructive: true,
                    confirm_label: format!("{} Delete", crate::icons::TRASH),
                },
            ) {
                ConfirmOutcome::Confirmed => {
                    self.pending_delete_bundle = None;
                    self.delete_bundle(&bundle_path);
                }
                ConfirmOutcome::Dismissed => self.pending_delete_bundle = None,
                ConfirmOutcome::Pending => {}
            }
        }

        // Clearing every animation is destructive and irreversible without
        // re-baking, so it confirms rather than riding on the Bake button.
        if self.pending_clear_animation {
            let count = self.model_clips.len();
            match render_confirm(
                ctx,
                &ConfirmSpec {
                    id: "clear_animation_backdrop",
                    title: "Clear Animation",
                    message: match count {
                        1 => "Remove 1 animation from this model?".to_string(),
                        n => format!("Remove all {n} animations from this model?"),
                    },
                    detail: Some(
                        "The rig and mesh are kept. Only the animation is removed. \
                         You can bake clips again afterwards."
                            .to_string(),
                    ),
                    destructive: true,
                    confirm_label: "Clear animation".to_string(),
                },
            ) {
                ConfirmOutcome::Confirmed => {
                    self.pending_clear_animation = false;
                    self.bake_set.clear();
                    if let Some(path) = self.current_model_path() {
                        self.start_bake(path, Vec::new());
                    }
                }
                ConfirmOutcome::Dismissed => self.pending_clear_animation = false,
                ConfirmOutcome::Pending => {}
            }
        }

        // Menu bar
        egui::Panel::top("menu_bar").show_inside(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Import Bundle...").clicked() {
                        // A folder can't be double-clicked in a file picker
                        // (that navigates), so the filter also accepts the
                        // folder's bundle.json — double-clicking THAT imports
                        // the folder. A loose .glb or image is wrapped into
                        // a library bundle. Drop onto Bundle Info does
                        // the same.
                        if let Some(path) = pick_bundle_import_file() {
                            self.import_bundle(path);
                        }
                        ui.close();
                    }
                    if ui.button("Import Bundle Folder...").clicked() {
                        // macOS note: single-click the folder, then Open —
                        // double-clicking navigates into it.
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            self.import_bundle(path);
                        }
                        ui.close();
                    }
                    if ui.button("Install Animation Pack...").clicked() {
                        if let Some(path) = pick_pack_install_file() {
                            self.install_clip_pack(path);
                        }
                        ui.close();
                    }
                    if ui.button("Install Animation Pack Folder...").clicked() {
                        if let Some(path) = pick_pack_install_folder() {
                            self.install_clip_pack(path);
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
                    let packs_downloading = self.clip_packs_downloading();
                    let packs_label = if packs_downloading {
                        clip_packs::DOWNLOAD_BUSY
                    } else {
                        clip_packs::DOWNLOAD_ACTION
                    };
                    if ui
                        .add_enabled(!packs_downloading, egui::Button::new(packs_label))
                        .on_hover_text(clip_packs::DOWNLOAD_HOVER)
                        .clicked()
                    {
                        self.request_clip_packs_download();
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
        egui::Panel::left("config_panel")
            .resizable(true)
            .default_size(320.0)
            .min_size(280.0)
            .show_inside(ui, |ui| {
                views::sidebar::render(self, ui);
            });

        // Bundle info panel - between sidebar and preview
        egui::Panel::left("bundle_info_panel")
            .resizable(true)
            .default_size(300.0)
            .min_size(250.0)
            .max_size(400.0)
            .show_inside(ui, |ui| {
                let drop_rect = ui.max_rect();
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
                self.drop_import_on_bundle_info(ui.ctx(), drop_rect);
            });

        // Bottom panel - progress
        egui::Panel::bottom("progress_panel")
            .resizable(true)
            .default_size(300.0)
            .min_size(100.0)
            .show_inside(ui, |ui| {
                views::progress::render(self, ui);
            });

        // Central panel - preview
        egui::CentralPanel::default().show_inside(ui, |ui| {
            views::preview::render(self, ui);
        });

        // Render library browser modal (if open)
        if let Some(selected_paths) = self.library_browser.render(ctx) {
            self.handle_library_selection(selected_paths);
        }

        // Consume any "Use this image for generation" request raised by the
        // library's context menu. This pipes a library image into the
        // existing_image sidebar slot without the user having to navigate via
        // the file picker.
        if let Some(path) = self.library_browser.pending_use_as_existing_image.take() {
            self.queue_image_for_generation(path.to_string_lossy().into_owned());
            self.library_browser.close();
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

        // After every zone has had its chance to claim this frame's drop.
        self.drop_unclaimed(ctx);

        // Request repaint while pipeline is running or toasts are visible.
        // Throttle to ~10 FPS — plenty for spinners/toasts — instead of
        // repainting at the display's max rate for the whole (minutes-long)
        // pipeline, which needlessly burns CPU/GPU.
        if self.state.lock().unwrap().running || !self.toasts.is_empty() || self.workbench_busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
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

/// How many clips a bake would add and remove.
///
/// Bake is declarative, so unticking a clip removes it. The panel states this
/// before the author commits, which is the only warning they get.
fn bake_delta_of(
    bake_set: &std::collections::BTreeSet<String>,
    model_clips: &std::collections::BTreeSet<String>,
) -> (usize, usize) {
    (
        bake_set.difference(model_clips).count(),
        model_clips.difference(bake_set).count(),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        App, PreviewTab, ToastType, WorkbenchJob, ZonelessRoute, bake_delta_of,
        build_startup_toasts, is_no_op_run, is_remote_url, pick_preview_tab_for_output,
        plan_loose_imports, route_without_zones, split_bake_set, workbench_completion_is_current,
    };
    use std::path::Path;

    #[test]
    fn stale_workbench_completions_are_rejected() {
        let job = WorkbenchJob {
            id: 3,
            model: PathBuf::from("/out/a/model.glb"),
        };
        // Same model, latest request: apply.
        assert!(workbench_completion_is_current(
            &job,
            Some(Path::new("/out/a/model.glb")),
            3
        ));
        // The user opened another bundle while it ran.
        assert!(!workbench_completion_is_current(
            &job,
            Some(Path::new("/out/b/model.glb")),
            3
        ));
        // Nothing is loaded any more (bundle deleted).
        assert!(!workbench_completion_is_current(&job, None, 3));
        // Same model, but a swap or a newer request bumped the sequence.
        assert!(!workbench_completion_is_current(
            &job,
            Some(Path::new("/out/a/model.glb")),
            4
        ));
    }

    #[test]
    fn bake_writes_only_what_an_installed_pack_can_supply() {
        // A model baked on another machine pre-ticks names no local pack
        // has. They must not count toward Bake, or an all-foreign set
        // enables the button and writes nothing — which is Clear.
        let ticked = set(&["Walk_Loop", "Foreign_Clip", "Idle_Loop"]);
        let catalog = ["Idle_Loop", "Walk_Loop", "Run_Loop"];
        let (writable, unavailable) = split_bake_set(&ticked, catalog);
        // Catalog order, so the file matches the list.
        assert_eq!(writable, ["Idle_Loop", "Walk_Loop"]);
        assert_eq!(unavailable, ["Foreign_Clip"]);

        let (writable, unavailable) = split_bake_set(&set(&["Foreign_Clip"]), catalog);
        assert!(writable.is_empty(), "nothing to bake");
        assert_eq!(unavailable, ["Foreign_Clip"]);

        let (writable, unavailable) = split_bake_set(&set(&[]), catalog);
        assert!(writable.is_empty());
        assert!(unavailable.is_empty());
    }

    #[test]
    fn loose_imports_pair_one_still_with_one_mesh_and_keep_the_rest() {
        let glbs = vec![PathBuf::from("a.glb"), PathBuf::from("b.glb")];
        let images = vec![PathBuf::from("x.png"), PathBuf::from("y.png")];
        let plan = plan_loose_imports(glbs, images);
        assert_eq!(
            plan,
            vec![
                vec![PathBuf::from("a.glb"), PathBuf::from("x.png")],
                vec![PathBuf::from("b.glb")],
                vec![PathBuf::from("y.png")],
            ]
        );
        // Every file lands in exactly one bundle.
        assert_eq!(plan.iter().map(Vec::len).sum::<usize>(), 4);
    }

    #[test]
    fn loose_imports_without_a_still_are_one_bundle_per_mesh() {
        let plan = plan_loose_imports(vec![PathBuf::from("a.glb"), PathBuf::from("b.glb")], vec![]);
        assert_eq!(
            plan,
            vec![vec![PathBuf::from("a.glb")], vec![PathBuf::from("b.glb")]]
        );
        assert!(plan_loose_imports(vec![], vec![]).is_empty());
    }

    #[test]
    fn zoneless_route_sends_a_lone_still_to_the_generation_slot() {
        // The pre-zones behavior, and the only way to reach that slot on a
        // platform that cannot report a drop position.
        let route = route_without_zones(vec![PathBuf::from("ref.png")], &[]);
        assert_eq!(
            route,
            ZonelessRoute {
                packs: vec![],
                generation_image: Some(PathBuf::from("ref.png")),
                new_bundle: vec![],
            }
        );
    }

    #[test]
    fn zoneless_route_pairs_a_still_with_a_mesh_into_one_bundle() {
        let route = route_without_zones(
            vec![PathBuf::from("ref.png"), PathBuf::from("hero.glb")],
            &[],
        );
        assert_eq!(
            route.generation_image, None,
            "the still belongs to the pair"
        );
        assert_eq!(
            route.new_bundle,
            vec![PathBuf::from("hero.glb"), PathBuf::from("ref.png")]
        );
    }

    #[test]
    fn zoneless_route_installs_packs_and_imports_the_rest() {
        let pack = PathBuf::from("Universal-Animation-Library.zip");
        let mesh = PathBuf::from("hero.glb");
        let route = route_without_zones(
            vec![pack.clone(), mesh.clone()],
            std::slice::from_ref(&pack),
        );
        assert_eq!(route.packs, vec![pack]);
        assert_eq!(route.new_bundle, vec![mesh]);
        assert_eq!(route.generation_image, None);
    }

    #[test]
    fn zoneless_route_never_drops_a_file_on_the_floor() {
        let files = vec![
            PathBuf::from("a.png"),
            PathBuf::from("b.jpg"),
            PathBuf::from("hero.glb"),
            PathBuf::from("ual1.zip"),
        ];
        let route = route_without_zones(files.clone(), &[PathBuf::from("ual1.zip")]);
        let routed = route.packs.len()
            + usize::from(route.generation_image.is_some())
            + route.new_bundle.len();
        assert_eq!(
            routed,
            files.len(),
            "every dropped path must land somewhere"
        );
    }

    #[test]
    fn bundle_drop_routing_predicate() {
        use std::path::Path;
        let tmp = tempfile::tempdir().unwrap();
        // Directories, zips, GLBs, and bundle.json route to bundle import
        // unless they look like a clip pack (checked first on drop).
        assert!(App::is_bundle_drop(tmp.path()));
        assert!(App::is_bundle_drop(Path::new("helmet.zip")));
        assert!(App::is_bundle_drop(Path::new("hero.glb")));
        assert!(App::is_bundle_drop(Path::new("run/bundle.json")));
        assert!(App::is_bundle_drop(Path::new("helmet.png")));
        // Arbitrary json does NOT.
        assert!(!App::is_bundle_drop(Path::new("settings.json")));
    }

    #[test]
    fn pack_drop_routing_predicate() {
        use std::path::Path;
        // Quaternius names and pack.json route to clip-pack install.
        assert!(App::is_pack_drop(Path::new(
            "Universal Animation Library 2[Source].zip"
        )));
        assert!(App::is_pack_drop(Path::new("UAL1_Standard.glb")));
        assert!(App::is_pack_drop(Path::new("clips/pack.json")));
        // A mesh / taphub bundle does not.
        assert!(!App::is_pack_drop(Path::new("hero.glb")));
        assert!(!App::is_pack_drop(Path::new("helmet.zip")));
        assert!(!App::is_pack_drop(Path::new("run/bundle.json")));
        assert!(!App::is_pack_drop(Path::new("helmet.png")));
    }

    #[test]
    fn image_file_predicate_matches_pipeline_formats() {
        use std::path::Path;
        for ok in ["a.png", "b.JPG", "c.jpeg", "d.webp", "e.gif", "f.avif"] {
            assert!(App::is_image_file(Path::new(ok)), "{ok}");
        }
        for no in ["m.glb", "z.zip", "bundle.json", "noext"] {
            assert!(!App::is_image_file(Path::new(no)), "{no}");
        }
    }
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
            "message must NOT claim changes won't be saved; they will. got {:?}",
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

    // =========================================================================
    // reconcile_provider_selection
    // =========================================================================

    use super::reconcile_provider_selection;
    use asset_tap_core::providers::{
        DynamicProvider, ProviderCapability, ProviderConfig, ProviderRegistry,
    };
    use std::sync::Arc;

    /// Build a minimal in-memory provider with the given id and one text-to-image
    /// model. Enough for `reconcile_provider_selection` to exercise the
    /// registry + capability + model lookups it cares about.
    fn make_test_provider(id: &str, model_id: &str) -> Arc<DynamicProvider> {
        use asset_tap_core::providers::config::{
            HttpMethod, ModelConfig, ProviderMetadataConfig, RequestTemplate, ResponseTemplate,
            ResponseType,
        };
        use std::collections::HashMap;
        let config = ProviderConfig {
            provider: ProviderMetadataConfig {
                upload: None,
                id: id.to_string(),
                name: format!("Test {}", id),
                description: "Test".to_string(),
                env_vars: vec!["TEST_KEY".to_string()],
                base_url: Some("https://example.com".to_string()),
                auth_format: None,
                api_key_url: None,
                website_url: None,
                docs_url: None,
                discovery: None,
            },
            text_to_image: vec![ModelConfig {
                id: model_id.to_string(),
                name: model_id.to_string(),
                description: "Test".to_string(),
                endpoint: "/test".to_string(),
                method: HttpMethod::POST,
                request: RequestTemplate {
                    headers: HashMap::new(),
                    body: None,
                    multipart: None,
                },
                response: ResponseTemplate {
                    response_type: ResponseType::Binary,
                    field: None,
                    polling: None,
                },
                is_default: true,
                parameters: vec![],
            }],
            image_to_3d: vec![],
        };
        Arc::new(DynamicProvider::new(config))
    }

    fn set(items: &[&str]) -> std::collections::BTreeSet<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn bake_delta_reports_both_directions() {
        let model = set(&["Walk_Loop", "Sword_Attack"]);
        // Untouched.
        assert_eq!(bake_delta_of(&model, &model), (0, 0));
        // Adding one.
        assert_eq!(
            bake_delta_of(&set(&["Walk_Loop", "Sword_Attack", "Idle_Loop"]), &model),
            (1, 0)
        );
        // Unticking one is a removal, which is the case an author can walk
        // into without noticing.
        assert_eq!(bake_delta_of(&set(&["Walk_Loop"]), &model), (0, 1));
        // Swapping is both at once.
        assert_eq!(bake_delta_of(&set(&["Idle_Loop"]), &model), (1, 2));
        // Clearing everything.
        assert_eq!(bake_delta_of(&set(&[]), &model), (0, 2));
    }

    /// Happy path: persisted selection still exists → return it unchanged.
    #[test]
    fn reconcile_keeps_valid_selection() {
        let mut registry = ProviderRegistry::empty();
        registry.register(make_test_provider("fal.ai", "flux-2"));
        let (p, m) = reconcile_provider_selection(
            &registry,
            ProviderCapability::TextToImage,
            "fal.ai".to_string(),
            "flux-2".to_string(),
            "fal.ai".to_string(),
        );
        assert_eq!(p, "fal.ai");
        assert_eq!(m, "flux-2");
    }

    /// Provider is still registered but the persisted model id is stale (e.g.
    /// renamed or removed). Keep the provider, swap in its default model.
    #[test]
    fn reconcile_swaps_stale_model_for_provider_default() {
        let mut registry = ProviderRegistry::empty();
        registry.register(make_test_provider("fal.ai", "flux-2"));
        let (p, m) = reconcile_provider_selection(
            &registry,
            ProviderCapability::TextToImage,
            "fal.ai".to_string(),
            "deleted-model".to_string(),
            "fal.ai".to_string(),
        );
        assert_eq!(p, "fal.ai");
        assert_eq!(
            m, "flux-2",
            "should fall back to the provider's default model"
        );
    }

    /// Provider disappeared entirely (e.g. mock mode hid it, YAML removed).
    /// Fall back to the registry default and its default model.
    #[test]
    fn reconcile_falls_back_when_provider_missing() {
        let mut registry = ProviderRegistry::empty();
        registry.register(make_test_provider("fal.ai", "flux-2"));
        let (p, m) = reconcile_provider_selection(
            &registry,
            ProviderCapability::TextToImage,
            "meshy".to_string(),
            "meshy/nano-banana".to_string(),
            "fal.ai".to_string(),
        );
        assert_eq!(p, "fal.ai");
        assert_eq!(m, "flux-2");
    }

    /// "Image only" plus an input image removes both stages, so the run is
    /// blocked rather than started. Neither selection is cleared for the user.
    #[test]
    fn image_only_with_input_image_is_a_no_op_run() {
        assert!(is_no_op_run(true, true));
    }

    #[test]
    fn remote_urls_are_not_checked_against_the_filesystem() {
        // A URL has no local path to stat; only picked files are validated.
        assert!(is_remote_url("https://example.com/a.png"));
        assert!(is_remote_url("http://example.com/a.png"));
        assert!(!is_remote_url("/Users/me/a.png"));
        assert!(!is_remote_url("relative/a.png"));
    }

    #[test]
    fn either_selection_alone_is_runnable() {
        assert!(!is_no_op_run(true, false), "image-only alone is valid");
        assert!(!is_no_op_run(false, true), "an input image alone is valid");
        assert!(!is_no_op_run(false, false));
    }
}
