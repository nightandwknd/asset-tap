//! Core traits and types for the provider plugin system.

use crate::types::{Progress, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Capabilities that a provider can support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProviderCapability {
    /// Text-to-image generation
    TextToImage,
    /// Image-to-3D model generation
    ImageTo3D,
}

/// Metadata about a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderMetadata {
    /// Provider identifier (from the 'id' field in provider config)
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Provider description
    pub description: String,
    /// Required environment variables for authentication
    pub required_env_vars: Vec<String>,
    /// Capabilities this provider supports
    pub capabilities: Vec<ProviderCapability>,
    /// URL where users can get API keys
    pub api_key_url: Option<String>,
    /// Provider website URL
    pub website_url: Option<String>,
    /// Provider documentation URL
    pub docs_url: Option<String>,
}

/// Information about a specific model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    /// Model identifier (e.g., "nano-banana", "sdxl")
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Model description
    pub description: Option<String>,
    /// Whether this is the default model for its capability
    pub is_default: bool,
    /// Provider-specific endpoint or version identifier
    pub endpoint: String,
    /// Additional provider-specific metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    /// Estimated cost per run in USD (from provider YAML config)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_per_run: Option<f64>,

    /// User-tunable parameters for this model (from provider YAML).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<crate::providers::config::ParameterDef>,
}

/// Result from image generation.
#[derive(Debug, Clone)]
pub struct ImageGenerationResult {
    /// Raw image data bytes
    pub data: Vec<u8>,
    /// Width of the image in pixels
    pub width: Option<u32>,
    /// Height of the image in pixels
    pub height: Option<u32>,
    /// Content type (e.g., "image/png")
    pub content_type: Option<String>,
}

/// Result from 3D model generation.
#[derive(Debug, Clone)]
pub struct Model3DGenerationResult {
    /// Raw model data bytes
    pub data: Vec<u8>,
    /// File format (e.g., "glb", "obj")
    pub format: Option<String>,
    /// Content type
    pub content_type: Option<String>,
}

/// The core provider trait that all providers must implement.
///
/// This trait defines a common interface for interacting with different
/// AI model providers.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Get provider metadata.
    fn metadata(&self) -> &ProviderMetadata;

    /// Downcast to `&dyn std::any::Any` for type checking.
    ///
    /// This enables downcasting to concrete provider types when needed
    /// (e.g., for accessing discovery-specific methods).
    fn as_any(&self) -> &dyn std::any::Any;

    /// Get the provider's unique identifier.
    fn id(&self) -> &str {
        &self.metadata().id
    }

    /// Get the provider's display name.
    fn name(&self) -> &str {
        &self.metadata().name
    }

    /// Check if this provider supports a given capability.
    fn supports(&self, capability: ProviderCapability) -> bool {
        self.metadata().capabilities.contains(&capability)
    }

    /// Check if the provider is available (API keys are configured).
    fn is_available(&self) -> bool;

    /// List all models for a given capability.
    fn list_models(&self, capability: ProviderCapability) -> Vec<ModelInfo>;

    /// Get information about a specific model.
    fn get_model(&self, model_id: &str) -> Result<ModelInfo> {
        // Default implementation searches through all capabilities
        for capability in &self.metadata().capabilities {
            let models = self.list_models(*capability);
            if let Some(model) = models.into_iter().find(|m| m.id == model_id) {
                return Ok(model);
            }
        }
        Err(crate::types::Error::InvalidModel(format!(
            "Model '{}' not found in provider '{}'",
            model_id,
            self.id()
        )))
    }

    /// Get the default model for a given capability.
    ///
    /// With dynamic discovery, there's no guaranteed "default" model.
    /// This returns the first available model, or an error if none exist.
    fn get_default_model(&self, capability: ProviderCapability) -> Result<ModelInfo> {
        let models = self.list_models(capability);

        // Try to find an explicitly marked default first
        if let Some(default) = models.iter().find(|m| m.is_default) {
            return Ok(default.clone());
        }

        // Otherwise, return the first available model
        models.into_iter().next().ok_or_else(|| {
            crate::types::Error::InvalidModel(format!(
                "No models available for {:?} in provider '{}'",
                capability,
                self.id()
            ))
        })
    }

    /// Generate an image from a text prompt.
    ///
    /// # Arguments
    /// * `prompt` - The text description
    /// * `model_id` - The model to use (must support TextToImage)
    /// * `params` - Optional parameter overrides (from GUI model settings panel)
    /// * `progress_tx` - Optional channel for progress updates
    async fn text_to_image(
        &self,
        prompt: &str,
        model_id: &str,
        params: Option<&HashMap<String, serde_json::Value>>,
        progress_tx: Option<tokio::sync::mpsc::UnboundedSender<Progress>>,
    ) -> Result<ImageGenerationResult>;

    /// Generate a 3D model from an image.
    ///
    /// # Arguments
    /// * `image_data` - Raw image data bytes
    /// * `model_id` - The model to use (must support ImageTo3D)
    /// * `params` - Optional parameter overrides (from GUI model settings panel)
    /// * `progress_tx` - Optional channel for progress updates
    async fn image_to_3d(
        &self,
        image_data: &[u8],
        model_id: &str,
        params: Option<&HashMap<String, serde_json::Value>>,
        progress_tx: Option<tokio::sync::mpsc::UnboundedSender<Progress>>,
    ) -> Result<Model3DGenerationResult>;
}
