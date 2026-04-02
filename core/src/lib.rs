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
//! - `mock` (default) - Enables mock API mode for testing without API costs

#![doc(html_root_url = "https://docs.rs/asset-tap-core/0.1.0")]

pub mod api;
pub mod bundle;
pub mod config;
pub mod config_version;
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

// Re-export commonly used types
pub use bundle::{
    Bundle, BundleContents, BundleError, BundleMetadata, ensure_default_bundles_exist,
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
