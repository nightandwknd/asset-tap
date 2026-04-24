//! Integration tests for the Asset Tap.
//!
//! These tests verify end-to-end behavior using the public API.
//! They run against the `asset-tap-core` crate as an external consumer would.

use asset_tap_core::{
    config::{generate_timestamp, list_image_to_3d_models, list_text_to_image_models},
    constants::files::bundle as bundle_files,
    pipeline::PipelineConfig,
    providers::ProviderRegistry,
    settings::Settings,
    state::AppState,
    templates::{apply_template, list_templates, template_exists},
    types::{ApiError, ApiErrorKind, ApiProvider, PipelineOutput, Stage},
};
use std::path::PathBuf;

// =============================================================================
// Pipeline Configuration Tests
// =============================================================================

#[test]
fn test_pipeline_config_builder_chain() {
    let config = PipelineConfig::new()
        .with_prompt(
            "a cowboy ninja with a leather duster, bandana mask, and dual katanas on the back",
        )
        .with_image_model("nano-banana")
        .with_3d_model("trellis-2");

    assert_eq!(
        config.prompt,
        Some(
            "a cowboy ninja with a leather duster, bandana mask, and dual katanas on the back"
                .to_string()
        )
    );
    assert_eq!(config.image_model, Some("nano-banana".to_string()));
    assert_eq!(config.model_3d, "trellis-2");
}

#[test]
fn test_pipeline_config_with_existing_image() {
    let config = PipelineConfig::new()
        .with_existing_image("https://example.com/image.png")
        .with_3d_model("trellis");

    assert!(config.prompt.is_none());
    assert_eq!(
        config.image_url,
        Some("https://example.com/image.png".to_string())
    );
}

// =============================================================================
// Template System Tests
// =============================================================================

#[test]
fn test_template_workflow() {
    // List available templates
    let templates = list_templates();
    assert!(!templates.is_empty());
    assert!(template_exists("humanoid"));

    // Apply a template to a base prompt
    let result = apply_template("humanoid", "a fierce warrior");
    assert!(result.is_some());

    let enhanced = result.unwrap();
    assert!(enhanced.contains("warrior"));
    // Template should add detail about humanoid proportions
    assert!(enhanced.len() > "a fierce warrior".len());
}

// =============================================================================
// Settings & State Tests
// =============================================================================

#[test]
fn test_settings_defaults() {
    let settings = Settings::default();

    // Provider API keys start as empty HashMap
    assert!(settings.provider_api_keys.is_empty());

    // Export FBX default is false
    assert!(!settings.export_fbx_default);
}

#[test]
fn test_app_state_round_trip() {
    let state = AppState {
        current_generation: Some(PathBuf::from("/output/test")),
        preview_tab: "Model3D".to_string(),
        sidebar_collapsed: true,
        last_prompt: Some("test prompt".to_string()),
        ..Default::default()
    };

    // Serialize and deserialize
    let json = serde_json::to_string(&state).unwrap();
    let restored: AppState = serde_json::from_str(&json).unwrap();

    assert_eq!(state.current_generation, restored.current_generation);
    assert_eq!(state.preview_tab, restored.preview_tab);
    assert_eq!(state.sidebar_collapsed, restored.sidebar_collapsed);
    assert_eq!(state.last_prompt, restored.last_prompt);
}

// =============================================================================
// Model Registry Tests
// =============================================================================

#[test]
fn test_model_registry() {
    // Set fake API key so provider is available
    unsafe { std::env::set_var("FAL_KEY", "test-key") };

    let registry = ProviderRegistry::new();

    // Should have image models
    let image_models = list_text_to_image_models(&registry);
    assert!(!image_models.is_empty());
    assert!(image_models.contains(&"fal-ai/nano-banana".to_string()));

    // Should have 3D models
    let models_3d = list_image_to_3d_models(&registry);
    assert!(!models_3d.is_empty());
    assert!(models_3d.contains(&"fal-ai/trellis-2".to_string()));

    // NOTE: Rigging models temporarily removed
    // let rig_models = list_rigging_models();
    // assert!(!rig_models.is_empty());
}

// =============================================================================
// Error Handling Tests
// =============================================================================

#[test]
fn test_api_error_classification() {
    // Test that errors are properly classified for different scenarios
    let scenarios = vec![
        (401, ApiErrorKind::Unauthorized, false),
        (402, ApiErrorKind::PaymentRequired, false),
        (429, ApiErrorKind::RateLimited, true),
        (500, ApiErrorKind::ServerError, true),
        (504, ApiErrorKind::Timeout, true),
    ];

    for (status, expected_kind, expected_retryable) in scenarios {
        let err = ApiError::from_response(ApiProvider::new("fal.ai"), status, "test", None);
        assert_eq!(
            err.kind, expected_kind,
            "Status {} should be {:?}",
            status, expected_kind
        );
        assert_eq!(
            err.retryable, expected_retryable,
            "Status {} retryable should be {}",
            status, expected_retryable
        );
    }
}

// =============================================================================
// Utility Tests
// =============================================================================

#[test]
fn test_timestamp_generation() {
    let ts1 = generate_timestamp();

    // Should follow format: YYYY-MM-DD_HHMMSS
    assert_eq!(ts1.len(), 17);
    assert!(ts1.contains('_'));

    // Wait for at least 1 second to ensure different timestamp
    std::thread::sleep(std::time::Duration::from_secs(1));
    let ts2 = generate_timestamp();

    // Timestamps should be unique (at second granularity)
    assert_ne!(ts1, ts2);
}

#[test]
fn test_pipeline_output_accessors() {
    let mut output = PipelineOutput::new();

    // Initially empty
    assert!(output.final_model_path().is_none());

    // With base model
    output.model_path = Some(PathBuf::from(bundle_files::MODEL_GLB));
    assert_eq!(
        output.final_model_path(),
        Some(&PathBuf::from(bundle_files::MODEL_GLB))
    );

    // Test that final_model_path returns the model_path
    assert_eq!(
        output.final_model_path(),
        Some(&PathBuf::from(bundle_files::MODEL_GLB))
    );
}

// =============================================================================
// Stage Display Tests
// =============================================================================

#[test]
fn test_all_stages_have_display_names() {
    let stages = vec![
        Stage::ImageGeneration,
        Stage::Model3DGeneration,
        Stage::FbxConversion,
        Stage::Download,
    ];

    for stage in stages {
        let display = stage.to_string();
        assert!(!display.is_empty());
        // Display names should be human-readable (contain spaces or be single words)
        assert!(
            display.contains(' ') || display.chars().all(|c| c.is_alphanumeric()),
            "Stage {:?} display '{}' should be human-readable",
            stage,
            display
        );
    }
}

// =============================================================================
// Config Version Tests
// =============================================================================

#[test]
fn test_embedded_config_sync_flow() {
    use asset_tap_core::config_sync::{SyncAction, determine_action, write_with_backup};
    use std::fs;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("provider.yaml");

    // Step 1: First run — file doesn't exist.
    let embedded_v1 = "provider:\n  id: test\n";
    assert_eq!(
        determine_action(embedded_v1, &config_path),
        SyncAction::WriteNew
    );
    write_with_backup(&config_path, embedded_v1, "provider").unwrap();
    assert!(config_path.exists());
    assert_eq!(fs::read_to_string(&config_path).unwrap(), embedded_v1);

    // Step 2: Second run with identical embedded content — no overwrite, no backup.
    assert_eq!(
        determine_action(embedded_v1, &config_path),
        SyncAction::UpToDate
    );
    let result = write_with_backup(&config_path, embedded_v1, "provider").unwrap();
    assert!(!result);
    assert!(!config_path.with_extension("yaml.bak").exists());

    // Step 3: Embedded content changed — overwrite and back up the old file.
    let embedded_v2 = "provider:\n  id: test\n  new_field: true\n";
    assert_eq!(
        determine_action(embedded_v2, &config_path),
        SyncAction::Overwrite
    );
    write_with_backup(&config_path, embedded_v2, "provider").unwrap();
    assert_eq!(fs::read_to_string(&config_path).unwrap(), embedded_v2);

    let backup = config_path.with_extension("yaml.bak");
    assert!(backup.exists());
    assert_eq!(fs::read_to_string(&backup).unwrap(), embedded_v1);
}

// =============================================================================
// Cross-provider parameter parity
// =============================================================================

/// Meshy v6 is served by two providers (fal's wrapper and Meshy's native API).
/// Users expect identical knobs regardless of which one they route through, and
/// these lists are intentionally duplicated in YAML because YAML sequences can't
/// be merged. This test is the drift-catcher: if someone adds a param to one
/// copy without the other, CI fails.
///
/// meshy-5 is included because it shares the same surface minus version-specific
/// deltas (none currently — just defaults differ).
#[test]
fn meshy_v6_parameter_surface_matches_across_providers() {
    use asset_tap_core::providers::config::ProviderConfig;
    use std::collections::BTreeSet;

    fn load(yaml_path: &str) -> ProviderConfig {
        let full =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../{}", yaml_path));
        ProviderConfig::from_yaml_file(&full)
            .unwrap_or_else(|e| panic!("loading {}: {}", full.display(), e))
    }

    fn param_names(config: &ProviderConfig, model_id: &str) -> BTreeSet<String> {
        config
            .image_to_3d
            .iter()
            .find(|m| m.id == model_id)
            .unwrap_or_else(|| panic!("model {} not found", model_id))
            .parameters
            .iter()
            .map(|p| p.name.clone())
            .collect()
    }

    let fal = load("providers/fal-ai.yaml");
    let meshy = load("providers/meshy.yaml");

    let fal_v6 = param_names(&fal, "fal-ai/meshy/v6/image-to-3d");
    let native_v6 = param_names(&meshy, "meshy/v6/image-to-3d");
    let native_v5 = param_names(&meshy, "meshy/v5/image-to-3d");

    assert_eq!(
        fal_v6,
        native_v6,
        "fal's Meshy v6 wrapper and Meshy's native v6 must expose the same \
         parameter names so users see identical knobs. Mismatch: only-in-fal={:?}, \
         only-in-native={:?}",
        fal_v6.difference(&native_v6).collect::<Vec<_>>(),
        native_v6.difference(&fal_v6).collect::<Vec<_>>()
    );

    assert_eq!(
        native_v6,
        native_v5,
        "Meshy v6 and v5 share the same parameter surface (only defaults differ). \
         Mismatch: only-in-v6={:?}, only-in-v5={:?}",
        native_v6.difference(&native_v5).collect::<Vec<_>>(),
        native_v5.difference(&native_v6).collect::<Vec<_>>()
    );
}

/// Every param declared in a model's `parameters:` list must also appear as a
/// key in the model's `request.body`, otherwise the runtime has nowhere to
/// inject the override and it silently becomes a no-op.
///
/// Catches typos like declaring `target_polycout` in the parameter list while
/// the body has `target_polycount`. We've hit this class of bug before — easy
/// to miss in review, impossible to miss in CI.
#[test]
fn every_declared_parameter_exists_in_request_body() {
    use asset_tap_core::providers::config::{ModelConfig, ProviderConfig};

    fn load(yaml_path: &str) -> ProviderConfig {
        let full =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../{}", yaml_path));
        ProviderConfig::from_yaml_file(&full)
            .unwrap_or_else(|e| panic!("loading {}: {}", full.display(), e))
    }

    fn check_model(provider_id: &str, model: &ModelConfig, missing: &mut Vec<String>) {
        let Some(body) = model.request.body.as_ref().and_then(|v| v.as_object()) else {
            // Models without a JSON body (multipart-only) have no keys to check.
            return;
        };
        for param in &model.parameters {
            if !body.contains_key(&param.name) {
                missing.push(format!(
                    "{} / {}: parameter '{}' is declared but not present in request.body",
                    provider_id, model.id, param.name
                ));
            }
        }
    }

    let mut missing = Vec::new();
    for path in ["providers/fal-ai.yaml", "providers/meshy.yaml"] {
        let config = load(path);
        let provider_id = config.provider.id.clone();
        for model in config.text_to_image.iter().chain(config.image_to_3d.iter()) {
            check_model(&provider_id, model, &mut missing);
        }
    }

    assert!(
        missing.is_empty(),
        "Parameter declarations out of sync with request bodies:\n  {}",
        missing.join("\n  ")
    );
}
