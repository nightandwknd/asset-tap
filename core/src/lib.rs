//! Asset Tap Core Library
//!
//! This library provides the core functionality for generating 3D models from text prompts
//! using AI providers. It follows a data-driven architecture with YAML-based provider plugins.
//!
//! # Overview
//!
//! The Asset Tap follows this flow:
//! 1. **Text → Image** - Generate image from text prompt using AI providers
//! 2. **Image → 3D Model** - Convert image to 3D model (GLB format)
//! 3. **GLB → FBX** - Optional export to FBX format using Blender
//!
//! # Quick Start
//!
//! ```no_run
//! use asset_tap_core::{PipelineConfig, pipeline::run_pipeline, providers::ProviderRegistry};
//!
//! # async fn example() -> anyhow::Result<()> {
//! // Create a pipeline configuration
//! let config = PipelineConfig::builder()
//!     .with_prompt("a cowboy ninja with a leather duster, bandana mask, and dual katanas on the back");
//!
//! // Create provider registry
//! let registry = ProviderRegistry::new();
//!
//! // Run the pipeline
//! let (mut progress_rx, handle, _approval_tx, _cancel_tx) = run_pipeline(config, &registry).await?;
//!
//! // Monitor progress
//! tokio::spawn(async move {
//!     while let Some(progress) = progress_rx.recv().await {
//!         println!("Progress: {:?}", progress);
//!     }
//! });
//!
//! // Wait for completion
//! let output = handle.await??;
//! if let Some(model_path) = output.model_path {
//!     println!("Generated model: {}", model_path.display());
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Core Components
//!
//! - **[`pipeline`]** - Pipeline orchestration and execution
//! - **[`providers`]** - Data-driven provider system for AI APIs
//! - **[`templates`]** - YAML-based prompt template system
//! - **[`config`]** - Configuration and directory utilities
//! - **[`settings`]** - User settings and persistence
//! - **[`bundle`]** - Output bundle metadata management
//! - **[`history`]** - Generation history tracking
//! - **[`types`]** - Core types, errors, and results
//!
//! # Architecture
//!
//! This library uses a **data-driven architecture**:
//! - Providers are loaded from YAML configs at runtime (not hardcoded)
//! - Templates use variable interpolation (`${variable}` syntax)
//! - Configs are embedded at compile-time but can be overridden by users
//!
//! # Feature Flags
//!
//! - `mock` (off by default) - Enables mock API mode for testing without API costs

pub mod api;
pub mod bundle;
mod bundle_schema;
pub mod config;
pub mod config_sync;
pub mod constants;
pub mod convert;
pub mod error_log;
pub mod glb_webp;
pub mod history;
pub mod pipeline;
pub mod progress_fmt;
pub mod providers;
pub mod settings;
pub mod state;
pub mod templates;
pub mod types;

/// Test-only synchronization for tests that mutate process-global state.
///
/// The provider/settings code reads API keys and mock flags from
/// `std::env::var(...)` at runtime, so any test that calls
/// `std::env::set_var`/`remove_var` races every other test that constructs a
/// provider or loads settings. Rather than forcing the *entire* suite to run
/// single-threaded, such tests take [`env_lock()`](test_support::env_lock) so
/// only they serialize while the pure majority runs in parallel.
///
/// This lives in the public API (doc-hidden) rather than behind `#[cfg(test)]`
/// so that integration tests in `core/tests/` — which compile against the
/// crate's public interface, where `cfg(test)` does not apply — can share the
/// *same* lock instance as the in-crate unit tests. The cost is one `Mutex`
/// static; it is never touched outside tests.
#[doc(hidden)]
pub mod test_support {
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());
    static TEMPLATES_DIR_LOCK: Mutex<()> = Mutex::new(());

    /// Acquire the process-wide lock guarding `std::env` mutation in tests.
    ///
    /// Hold the returned guard for the duration of any test that sets or
    /// removes environment variables. The guard is poison-tolerant: a panicking
    /// test still releases the lock for the next one.
    pub fn env_lock() -> MutexGuard<'static, ()> {
        ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Acquire the lock guarding the shared user-templates directory in tests.
    ///
    /// `TemplateRegistry::new()` writes the embedded templates into the shared
    /// on-disk templates dir; concurrent writers race on those files. Tests that
    /// construct a registry via `new()` (rather than the isolated
    /// `from_dir(tempdir)`) hold this so they serialize against each other while
    /// the rest of the suite runs in parallel.
    pub fn templates_dir_lock() -> MutexGuard<'static, ()> {
        TEMPLATES_DIR_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

// Re-export commonly used types
pub use bundle::{
    Bundle, BundleContents, BundleError, BundleMetadata, DemoDownloadResult, download_demo_bundle,
    import_bundle_dir, import_bundle_zip,
};
pub use config::{list_image_to_3d_models, list_text_to_image_models};
pub use error_log::ErrorLog;
pub use history::{GenerationHistory, GenerationRecord, GenerationStatus};
pub use pipeline::{PipelineConfig, run_pipeline};
pub use progress_fmt::{DisplayLevel, ProgressDisplay, format_progress};
pub use settings::Settings;
pub use state::AppState;
pub use types::{
    ApiError, ApiErrorKind, ApiProvider, ApprovalResponse, Error, PipelineOutput, Progress, Result,
    Stage,
};
