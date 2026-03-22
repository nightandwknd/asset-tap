//! Provider plugin system.
//!
//! This module defines the YAML/JSON-based plugin system that allows
//! providers to be configured without code changes. This is a **data-driven
//! architecture** where AI providers are loaded from configuration files
//! rather than being hardcoded.
//!
//! # Architecture
//!
//! - [`Provider`] trait: Common interface all providers must implement
//! - [`ProviderRegistry`]: Discovers and loads providers from YAML/JSON configs
//! - [`DynamicProvider`]: Runtime provider implementation from config files
//! - `HttpProviderClient`: Generic HTTP client for executing provider configs
//!
//! # Provider Locations
//!
//! Providers are loaded from:
//!
//! 1. **Embedded providers**: All `providers/*.yaml` files are compiled into the binary
//! 2. **User providers**: `.dev/providers/*.yaml` (dev mode) or `~/.config/asset-tap/providers/` (release)
//!
//! On first run, embedded providers are written to the user directory where they can be edited or removed.
//! User providers can override built-in ones by using the same `id` field.
//!
//! # Quick Start
//!
//! ```no_run
//! use asset_tap_core::providers::{ProviderRegistry, ProviderCapability};
//!
//! // Load all providers
//! let registry = ProviderRegistry::new();
//!
//! // Get first available provider with text-to-image capability
//! let provider = registry.get_by_capability(ProviderCapability::TextToImage)
//!     .expect("No text-to-image providers available");
//!
//! // List available models
//! let models = provider.list_models(ProviderCapability::TextToImage);
//! for model in models {
//!     println!("{}: {}", model.id, model.name);
//! }
//! ```
//!
//! # See Also
//!
//! - [`pipeline`](crate::pipeline) - Pipeline execution using providers
//! - [`config::ProviderConfig`] - Provider YAML configuration format

pub mod config;
pub mod discovery;
pub mod discovery_cache;
pub mod dynamic_provider;
pub mod http_client;
pub mod openapi;
pub mod registry;
pub mod traits;

pub use config::ProviderConfig;
pub use dynamic_provider::DynamicProvider;
pub use registry::ProviderRegistry;
pub use traits::{
    ImageGenerationResult, Model3DGenerationResult, ModelInfo, Provider, ProviderCapability,
    ProviderMetadata,
};
