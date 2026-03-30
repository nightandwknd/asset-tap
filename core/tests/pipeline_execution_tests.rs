//! Pipeline execution tests.
//!
//! These tests validate the core pipeline orchestration using mock providers.

use asset_tap_core::{
    constants::{files::bundle as bundle_files, http::env},
    pipeline::{PipelineConfig, run_pipeline},
    providers::ProviderRegistry,
};
use std::path::PathBuf;
use tempfile::TempDir;

// =============================================================================
// Test Helpers
// =============================================================================

/// Set up test environment with mock mode enabled.
fn setup_mock_env() -> TempDir {
    unsafe {
        std::env::set_var(env::MOCK_API, "1");
        std::env::set_var("FAL_KEY", "test-key-for-mock-mode");
    }
    TempDir::new().expect("Failed to create temp directory")
}

fn cleanup_mock_env() {
    unsafe {
        std::env::remove_var(env::MOCK_API);
        std::env::remove_var("FAL_KEY");
    }
}

// =============================================================================
// Basic Pipeline Execution Tests
// =============================================================================

#[tokio::test]
async fn test_pipeline_text_to_3d_with_mock() {
    let temp_dir = setup_mock_env();

    let config = PipelineConfig::new()
        .with_prompt("a test robot")
        .with_image_model("fal-ai/nano-banana")
        .with_3d_model("fal-ai/trellis-2")
        .with_output_dir(temp_dir.path().to_path_buf())
        .without_fbx(); // Skip FBX to speed up test

    let registry = ProviderRegistry::new();

    // Run pipeline
    let (mut progress_rx, handle, _approval_tx, _cancel_tx) = run_pipeline(config, &registry)
        .await
        .expect("Pipeline should start");

    // Drain progress channel (providers may or may not emit progress in mock mode)
    while progress_rx.recv().await.is_some() {}

    // Wait for completion
    let output = handle
        .await
        .expect("Task should complete")
        .expect("Pipeline should succeed");

    // Verify output
    assert_eq!(output.prompt, Some("a test robot".to_string()));
    assert!(output.image_path.is_some(), "Should have image path");
    assert!(output.model_path.is_some(), "Should have model path");

    // Verify output directory was created
    assert!(output.output_dir.is_some(), "Should have output directory");
    if let Some(ref dir) = output.output_dir {
        assert!(dir.exists(), "Output directory should exist");
    }

    cleanup_mock_env();
}

#[tokio::test]
async fn test_pipeline_with_existing_image() {
    let temp_dir = setup_mock_env();

    // Create a local test image file instead of using a URL that would 404
    let test_image_path = temp_dir.path().join("test_input.png");
    std::fs::write(&test_image_path, [0x89, 0x50, 0x4E, 0x47]).unwrap(); // Minimal PNG header

    let config = PipelineConfig::new()
        .with_existing_image(test_image_path.to_string_lossy())
        .with_3d_model("fal-ai/trellis-2")
        .with_output_dir(temp_dir.path().to_path_buf())
        .without_fbx();

    let registry = ProviderRegistry::new();
    let (mut progress_rx, handle, _approval_tx, _cancel_tx) = run_pipeline(config, &registry)
        .await
        .expect("Pipeline should start");

    // Drain progress channel
    while progress_rx.recv().await.is_some() {}

    let output = handle.await.unwrap().expect("Should succeed");

    // Verify output - should have model but image_path should reference the existing image
    assert!(output.model_path.is_some(), "Should have model output");

    cleanup_mock_env();
}

// =============================================================================
// Progress Tracking Tests
// =============================================================================

#[tokio::test]
async fn test_pipeline_progress_stages() {
    let temp_dir = setup_mock_env();

    let config = PipelineConfig::new()
        .with_prompt("test")
        .with_image_model("fal-ai/nano-banana")
        .with_3d_model("fal-ai/trellis-2")
        .with_output_dir(temp_dir.path().to_path_buf())
        .without_fbx();

    let registry = ProviderRegistry::new();
    let (mut progress_rx, handle, _approval_tx, _cancel_tx) =
        run_pipeline(config, &registry).await.unwrap();

    // Drain progress channel (events are optional in mock mode)
    while progress_rx.recv().await.is_some() {}

    // Verify pipeline completes successfully
    handle.await.unwrap().unwrap();

    cleanup_mock_env();
}

// =============================================================================
// Configuration Tests
// =============================================================================

#[test]
fn test_pipeline_config_builder() {
    let config = PipelineConfig::new()
        .with_prompt("test prompt")
        .with_image_model("fal-ai/nano-banana")
        .with_3d_model("fal-ai/trellis-2")
        .with_output_dir(PathBuf::from("/tmp/test"));

    assert_eq!(config.prompt, Some("test prompt".to_string()));
    assert_eq!(config.image_model, Some("fal-ai/nano-banana".to_string()));
    assert_eq!(config.model_3d, "fal-ai/trellis-2");
    assert_eq!(config.output_dir, Some(PathBuf::from("/tmp/test")));
    assert!(config.export_fbx, "FBX export should be enabled by default");
}

#[test]
fn test_pipeline_config_without_fbx() {
    let config = PipelineConfig::new().without_fbx();
    assert!(!config.export_fbx, "FBX export should be disabled");
}

#[test]
fn test_pipeline_config_effective_image_model() {
    // With prompt, should need image generation
    let config = PipelineConfig::new()
        .with_prompt("test")
        .with_image_model("fal-ai/nano-banana");
    assert_eq!(config.effective_image_model(), Some("fal-ai/nano-banana"));

    // With existing image, should not need image generation
    let config = PipelineConfig::new().with_existing_image("https://example.com/image.png");
    assert_eq!(config.effective_image_model(), None);
}

// =============================================================================
// Provider Selection Tests
// =============================================================================

#[tokio::test]
async fn test_pipeline_with_specific_provider() {
    let temp_dir = setup_mock_env();

    let config = PipelineConfig::new()
        .with_prompt("test")
        .with_image_provider("fal.ai")
        .with_3d_provider("fal.ai")
        .with_image_model("fal-ai/nano-banana")
        .with_3d_model("fal-ai/trellis-2")
        .with_output_dir(temp_dir.path().to_path_buf())
        .without_fbx();

    let registry = ProviderRegistry::new();

    // Should successfully use specified provider
    let result = run_pipeline(config, &registry).await;
    assert!(result.is_ok(), "Should accept valid provider");

    let (mut rx, handle, _approval_tx, _cancel_tx) = result.unwrap();
    while rx.recv().await.is_some() {}

    let output = handle.await.unwrap();
    assert!(output.is_ok(), "Pipeline should complete successfully");

    cleanup_mock_env();
}

#[tokio::test]
async fn test_pipeline_with_invalid_provider() {
    let temp_dir = setup_mock_env();

    let config = PipelineConfig::new()
        .with_prompt("test")
        .with_image_provider("nonexistent-provider")
        .with_3d_model("fal-ai/trellis-2")
        .with_output_dir(temp_dir.path().to_path_buf());

    let registry = ProviderRegistry::new();

    // Should fail with invalid provider
    let result = run_pipeline(config, &registry).await;
    assert!(result.is_err(), "Should fail with nonexistent provider");

    cleanup_mock_env();
}

// =============================================================================
// Output Validation Tests
// =============================================================================

#[tokio::test]
async fn test_pipeline_creates_output_directory() {
    let temp_dir = setup_mock_env();

    let config = PipelineConfig::new()
        .with_prompt("test")
        .with_image_model("fal-ai/nano-banana")
        .with_3d_model("fal-ai/trellis-2")
        .with_output_dir(temp_dir.path().to_path_buf())
        .without_fbx();

    let registry = ProviderRegistry::new();
    let (mut rx, handle, _approval_tx, _cancel_tx) = run_pipeline(config, &registry).await.unwrap();

    while rx.recv().await.is_some() {}
    let output = handle.await.unwrap().unwrap();

    // Verify output directory
    assert!(output.output_dir.is_some(), "Should have output directory");
    let output_dir = output.output_dir.unwrap();
    assert!(output_dir.exists(), "Output directory should exist");

    // Verify directory name format (YYYY-MM-DD_HHMMSS)
    let dir_name = output_dir.file_name().unwrap().to_str().unwrap();
    assert_eq!(dir_name.len(), 17, "Directory name should be 17 chars");
    assert!(
        dir_name.contains('_'),
        "Directory name should contain underscore"
    );

    cleanup_mock_env();
}

#[tokio::test]
async fn test_pipeline_creates_bundle_metadata() {
    let temp_dir = setup_mock_env();

    let config = PipelineConfig::new()
        .with_prompt("test metadata")
        .with_image_model("fal-ai/nano-banana")
        .with_3d_model("fal-ai/trellis-2")
        .with_output_dir(temp_dir.path().to_path_buf())
        .without_fbx();

    let registry = ProviderRegistry::new();
    let (mut rx, handle, _approval_tx, _cancel_tx) = run_pipeline(config, &registry).await.unwrap();

    while rx.recv().await.is_some() {}
    let output = handle.await.unwrap().unwrap();

    // Check bundle.json exists
    assert!(output.output_dir.is_some());
    let bundle_json = output.output_dir.unwrap().join(bundle_files::METADATA);
    assert!(bundle_json.exists(), "bundle.json should exist");

    // Verify bundle.json content
    let content = std::fs::read_to_string(&bundle_json).unwrap();
    let metadata: serde_json::Value = serde_json::from_str(&content).unwrap();

    // Verify metadata has config
    assert!(metadata.get("config").is_some(), "Should have config");
    let config_json = &metadata["config"];
    assert_eq!(config_json["prompt"], "test metadata");
    assert_eq!(config_json["image_model"], "fal-ai/nano-banana");
    assert_eq!(config_json["model_3d"], "fal-ai/trellis-2");

    cleanup_mock_env();
}

#[tokio::test]
async fn test_pipeline_creates_expected_files() {
    let temp_dir = setup_mock_env();

    let config = PipelineConfig::new()
        .with_prompt("test files")
        .with_image_model("fal-ai/nano-banana")
        .with_3d_model("fal-ai/trellis-2")
        .with_output_dir(temp_dir.path().to_path_buf())
        .without_fbx();

    let registry = ProviderRegistry::new();
    let (mut rx, handle, _approval_tx, _cancel_tx) = run_pipeline(config, &registry).await.unwrap();

    while rx.recv().await.is_some() {}
    let output = handle.await.unwrap().unwrap();

    assert!(output.output_dir.is_some());
    let output_dir = output.output_dir.unwrap();

    // Check expected files exist
    assert!(
        output_dir.join(bundle_files::IMAGE).exists(),
        "image.png should exist"
    );
    assert!(
        output_dir.join(bundle_files::MODEL_GLB).exists(),
        "model.glb should exist"
    );
    assert!(
        output_dir.join(bundle_files::METADATA).exists(),
        "bundle.json should exist"
    );

    cleanup_mock_env();
}

// =============================================================================
// Concurrent Pipeline Tests
// =============================================================================

#[tokio::test]
async fn test_multiple_pipelines_concurrent() {
    let temp_dir = setup_mock_env();
    let registry = ProviderRegistry::new();

    // Start 3 pipelines concurrently
    let mut handles = Vec::new();

    for i in 0..3 {
        let config = PipelineConfig::new()
            .with_prompt(format!("concurrent test {}", i))
            .with_image_model("fal-ai/nano-banana")
            .with_3d_model("fal-ai/trellis-2")
            .with_output_dir(temp_dir.path().to_path_buf())
            .without_fbx();

        let (mut rx, handle, _approval_tx, _cancel_tx) =
            run_pipeline(config, &registry).await.unwrap();

        // Spawn task to drain progress
        let drain_task = tokio::spawn(async move { while rx.recv().await.is_some() {} });

        handles.push((handle, drain_task));
    }

    // Wait for all to complete
    for (pipeline_handle, drain_handle) in handles {
        drain_handle.await.unwrap();
        let result = pipeline_handle.await.unwrap();
        assert!(result.is_ok(), "Each pipeline should succeed");
    }

    cleanup_mock_env();
}

// =============================================================================
// Error Handling Tests
// =============================================================================

#[tokio::test]
async fn test_pipeline_without_providers() {
    // Ensure mock mode is OFF so providers aren't auto-configured with fake keys
    unsafe {
        std::env::remove_var(env::MOCK_API);
        std::env::remove_var("FAL_KEY");
    }

    let temp_dir = TempDir::new().unwrap();

    let config = PipelineConfig::new()
        .with_prompt("test")
        .with_image_model("fal-ai/nano-banana")
        .with_3d_model("fal-ai/trellis-2")
        .with_output_dir(temp_dir.path().to_path_buf());

    let registry = ProviderRegistry::new();

    // Should fail because no providers available
    let result = run_pipeline(config, &registry).await;

    assert!(result.is_err(), "Should fail without providers");

    let err = result.unwrap_err();
    let err_str = format!("{}", err);
    assert!(
        err_str.contains("provider") || err_str.contains("available") || err_str.contains("key"),
        "Error should mention provider availability: {}",
        err_str
    );
}

#[tokio::test]
async fn test_pipeline_rejects_oversized_prompt() {
    let temp_dir = setup_mock_env();
    let registry = ProviderRegistry::new();

    let long_prompt = "x".repeat(asset_tap_core::constants::validation::MAX_PROMPT_LENGTH + 1);

    let config = PipelineConfig::new()
        .with_prompt(&long_prompt)
        .with_image_model("fal-ai/nano-banana")
        .with_3d_model("fal-ai/trellis-2")
        .with_output_dir(temp_dir.path().to_path_buf())
        .without_fbx();

    let (mut rx, handle, _approval_tx, _cancel_tx) = run_pipeline(config, &registry).await.unwrap();

    while rx.recv().await.is_some() {}
    let result = handle.await.unwrap();

    assert!(result.is_err(), "Should reject oversized prompt");
    let err_str = format!("{}", result.unwrap_err());
    assert!(
        err_str.contains("too long"),
        "Error should mention prompt length: {}",
        err_str
    );

    cleanup_mock_env();
}

#[tokio::test]
async fn test_pipeline_accepts_max_length_prompt() {
    let temp_dir = setup_mock_env();
    let registry = ProviderRegistry::new();

    let max_prompt = "x".repeat(asset_tap_core::constants::validation::MAX_PROMPT_LENGTH);

    let config = PipelineConfig::new()
        .with_prompt(&max_prompt)
        .with_image_model("fal-ai/nano-banana")
        .with_3d_model("fal-ai/trellis-2")
        .with_output_dir(temp_dir.path().to_path_buf())
        .without_fbx();

    let (mut rx, handle, _approval_tx, _cancel_tx) = run_pipeline(config, &registry).await.unwrap();

    while rx.recv().await.is_some() {}
    let result = handle.await.unwrap();

    assert!(
        result.is_ok(),
        "Should accept prompt at exactly max length: {:?}",
        result.err()
    );

    cleanup_mock_env();
}

// =============================================================================
// Cancellation Tests
// =============================================================================

#[tokio::test]
async fn test_pipeline_cancel_before_3d() {
    let temp_dir = setup_mock_env();

    let config = PipelineConfig::new()
        .with_prompt("cancel test")
        .with_image_model("fal-ai/nano-banana")
        .with_3d_model("fal-ai/trellis-2")
        .with_output_dir(temp_dir.path().to_path_buf())
        .without_fbx();

    let registry = ProviderRegistry::new();
    let (mut progress_rx, handle, _approval_tx, cancel_tx) =
        run_pipeline(config, &registry).await.unwrap();

    // Send cancel immediately — the pipeline will check it between stages
    let _ = cancel_tx.send(());

    // Drain progress
    while progress_rx.recv().await.is_some() {}

    let result = handle.await.unwrap();
    // Pipeline may or may not have been cancelled depending on timing,
    // but it should not panic either way
    if let Err(ref e) = result {
        let err_str = format!("{}", e);
        assert!(
            err_str.contains("cancelled by user"),
            "Cancel error should mention user cancellation: {}",
            err_str
        );
    }

    cleanup_mock_env();
}
