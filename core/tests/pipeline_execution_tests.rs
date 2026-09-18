#![cfg(feature = "mock")]
// These tests deliberately hold the shared env-lock guard across `.await` to
// serialize env mutation for the whole test body — the intended pattern here.
// `#[tokio::test]`'s current-thread runtime means the `!Send` guard is sound.
#![allow(clippy::await_holding_lock)]
//! Pipeline execution tests.
//!
//! These tests validate the core pipeline orchestration using mock providers.

use asset_tap_core::{
    constants::{files::bundle as bundle_files, http::env},
    pipeline::{PipelineConfig, run_pipeline},
    providers::{ProviderCapability, ProviderRegistry},
};
use std::path::PathBuf;
use tempfile::TempDir;

// =============================================================================
// Test Helpers
// =============================================================================

/// Set up test environment with mock mode enabled.
///
/// Returns the shared env-lock guard alongside the temp dir. Callers must bind
/// **both** (e.g. `let (_env, temp) = setup_mock_env();`) and hold the guard for
/// the whole test — it serializes this test against every other env-mutating
/// test so the suite can otherwise run in parallel. The guard is `!Send`, which
/// is fine under `#[tokio::test]`'s default current-thread runtime.
fn setup_mock_env() -> (std::sync::MutexGuard<'static, ()>, TempDir) {
    let guard = asset_tap_core::test_support::env_lock();
    unsafe {
        std::env::set_var(env::MOCK_API, "1");
        std::env::set_var("FAL_KEY", "test-key-for-mock-mode");
    }
    let temp = TempDir::new().expect("Failed to create temp directory");
    (guard, temp)
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
    let (_env, temp_dir) = setup_mock_env();

    let config = PipelineConfig::new()
        .with_prompt("a test robot")
        .with_image_model("fal-ai/nano-banana")
        .with_3d_model("fal-ai/trellis-2")
        .with_output_dir(temp_dir.path().to_path_buf());

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
    let (_env, temp_dir) = setup_mock_env();

    // Create a local test image file instead of using a URL that would 404
    let test_image_path = temp_dir.path().join("test_input.png");
    std::fs::write(&test_image_path, [0x89, 0x50, 0x4E, 0x47]).unwrap(); // Minimal PNG header

    let config = PipelineConfig::new()
        .with_existing_image(test_image_path.to_string_lossy())
        .with_3d_model("fal-ai/trellis-2")
        .with_output_dir(temp_dir.path().to_path_buf());

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
    let (_env, temp_dir) = setup_mock_env();

    let config = PipelineConfig::new()
        .with_prompt("test")
        .with_image_model("fal-ai/nano-banana")
        .with_3d_model("fal-ai/trellis-2")
        .with_output_dir(temp_dir.path().to_path_buf());

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
    let (_env, temp_dir) = setup_mock_env();

    let config = PipelineConfig::new()
        .with_prompt("test")
        .with_image_provider("fal.ai")
        .with_3d_provider("fal.ai")
        .with_image_model("fal-ai/nano-banana")
        .with_3d_model("fal-ai/trellis-2")
        .with_output_dir(temp_dir.path().to_path_buf());

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
    let (_env, temp_dir) = setup_mock_env();

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
    let (_env, temp_dir) = setup_mock_env();

    let config = PipelineConfig::new()
        .with_prompt("test")
        .with_image_model("fal-ai/nano-banana")
        .with_3d_model("fal-ai/trellis-2")
        .with_output_dir(temp_dir.path().to_path_buf());

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
    let (_env, temp_dir) = setup_mock_env();

    let config = PipelineConfig::new()
        .with_prompt("test metadata")
        .with_image_model("fal-ai/nano-banana")
        .with_3d_model("fal-ai/trellis-2")
        .with_output_dir(temp_dir.path().to_path_buf());

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

    assert!(metadata.get("config").is_none(), "v2 writes omit config");
    assert!(
        metadata.get("model_info").is_none(),
        "v2 writes omit model_info"
    );

    assert_eq!(metadata["version"], 2);
    assert_eq!(metadata["primary"], "model");
    assert!(metadata.get("category").is_none());
    let artifacts = metadata["artifacts"].as_array().expect("artifacts");
    assert!(artifacts.iter().any(|a| a["id"] == "image"));
    assert!(artifacts.iter().any(|a| a["id"] == "model"));
    let steps = metadata["pipeline"]["steps"].as_array().expect("steps");
    assert_eq!(steps[0]["kind"], "model");
    assert_eq!(steps[0]["modality"], "text_to_image");
    assert_eq!(steps[0]["prompt"], "test metadata");
    assert_eq!(steps[0]["model"], "fal-ai/nano-banana");
    assert_eq!(steps[1]["kind"], "model");
    assert_eq!(steps[1]["modality"], "image_to_3d");
    assert_eq!(steps[1]["model"], "fal-ai/trellis-2");

    cleanup_mock_env();
}

#[tokio::test]
async fn test_pipeline_image_only_writes_v2_without_model_step() {
    let (_env, temp_dir) = setup_mock_env();

    let config = PipelineConfig::new()
        .with_prompt("test image only")
        .with_image_model("fal-ai/nano-banana")
        .with_skip_3d()
        .with_output_dir(temp_dir.path().to_path_buf());

    let registry = ProviderRegistry::new();
    let (mut rx, handle, _approval_tx, _cancel_tx) = run_pipeline(config, &registry).await.unwrap();

    while rx.recv().await.is_some() {}
    let output = handle.await.unwrap().unwrap();

    let bundle_json = output.output_dir.unwrap().join(bundle_files::METADATA);
    let metadata: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&bundle_json).unwrap()).unwrap();

    assert_eq!(metadata["version"], 2);
    assert_eq!(metadata["primary"], "image");
    assert!(metadata.get("category").is_none() || metadata["category"].is_null());
    let artifacts = metadata["artifacts"].as_array().expect("artifacts");
    assert!(artifacts.iter().any(|a| a["id"] == "image"));
    assert!(!artifacts.iter().any(|a| a["id"] == "model"));
    let steps = metadata["pipeline"]["steps"].as_array().expect("steps");
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0]["modality"], "text_to_image");

    cleanup_mock_env();
}

#[tokio::test]
async fn test_pipeline_creates_expected_files() {
    let (_env, temp_dir) = setup_mock_env();

    let config = PipelineConfig::new()
        .with_prompt("test files")
        .with_image_model("fal-ai/nano-banana")
        .with_3d_model("fal-ai/trellis-2")
        .with_output_dir(temp_dir.path().to_path_buf());

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
    let (_env, temp_dir) = setup_mock_env();
    let registry = ProviderRegistry::new();

    // Start 3 pipelines concurrently
    let mut handles = Vec::new();

    for i in 0..3 {
        let config = PipelineConfig::new()
            .with_prompt(format!("concurrent test {}", i))
            .with_image_model("fal-ai/nano-banana")
            .with_3d_model("fal-ai/trellis-2")
            .with_output_dir(temp_dir.path().to_path_buf());

        let (mut rx, handle, _approval_tx, _cancel_tx) =
            run_pipeline(config, &registry).await.unwrap();

        // Spawn task to drain progress
        let drain_task = tokio::spawn(async move { while rx.recv().await.is_some() {} });

        handles.push((handle, drain_task));
    }

    // Wait for all to complete
    for (i, (pipeline_handle, drain_handle)) in handles.into_iter().enumerate() {
        drain_handle.await.unwrap();
        let result = pipeline_handle.await.unwrap();
        assert!(
            result.is_ok(),
            "Pipeline {} should succeed, got: {:?}",
            i,
            result.err()
        );
    }

    cleanup_mock_env();
}

// =============================================================================
// Error Handling Tests
// =============================================================================

#[tokio::test]
async fn test_pipeline_without_providers() {
    // Hold the shared env-lock guard for the whole test body (through the
    // `.await` and all assertions) — this test mutates process-global env vars
    // just like `setup_mock_env()` does, so it must serialize against every
    // other env-mutating test the same way they serialize against each other.
    // Without this, an unguarded `remove_var("FAL_KEY")` here can race another
    // test's live `is_configured()` env read and intermittently fail it.
    let _env = asset_tap_core::test_support::env_lock();

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
    let (_env, temp_dir) = setup_mock_env();
    let registry = ProviderRegistry::new();

    let long_prompt = "x".repeat(asset_tap_core::constants::validation::MAX_PROMPT_LENGTH + 1);

    let config = PipelineConfig::new()
        .with_prompt(&long_prompt)
        .with_image_model("fal-ai/nano-banana")
        .with_3d_model("fal-ai/trellis-2")
        .with_output_dir(temp_dir.path().to_path_buf());

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
    let (_env, temp_dir) = setup_mock_env();
    let registry = ProviderRegistry::new();

    let max_prompt = "x".repeat(asset_tap_core::constants::validation::MAX_PROMPT_LENGTH);

    let config = PipelineConfig::new()
        .with_prompt(&max_prompt)
        .with_image_model("fal-ai/nano-banana")
        .with_3d_model("fal-ai/trellis-2")
        .with_output_dir(temp_dir.path().to_path_buf());

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
    let (_env, temp_dir) = setup_mock_env();

    let config = PipelineConfig::new()
        .with_prompt("cancel test")
        .with_image_model("fal-ai/nano-banana")
        .with_3d_model("fal-ai/trellis-2")
        .with_output_dir(temp_dir.path().to_path_buf());

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

// =============================================================================
// Mock coverage guarantee
// =============================================================================

/// Every registered provider must complete a full pipeline in mock mode.
///
/// Mock handlers are synthesized from each provider's own YAML polling contract
/// (see `api::mock::config_driven`). Add a provider whose shape the synthesizer
/// can't build and this test fails, rather than the provider silently
/// disappearing from mock mode or breaking at runtime.
#[tokio::test]
async fn test_every_provider_runs_in_mock_mode() {
    let (_env, temp_dir) = setup_mock_env();

    let registry = ProviderRegistry::new();
    let providers = registry.list_all();
    assert!(!providers.is_empty(), "No providers registered");

    let mut exercised = Vec::new();

    for provider in &providers {
        let id = provider.id().to_string();
        let image_model = provider.get_default_model(ProviderCapability::TextToImage);
        let model_3d = provider.get_default_model(ProviderCapability::ImageTo3D);

        // A provider need not offer both capabilities; run whatever it declares.
        let (Ok(image_model), Ok(model_3d)) = (image_model, model_3d) else {
            continue;
        };

        let out_dir = temp_dir.path().join(id.replace('.', "_"));
        let config = PipelineConfig::new()
            .with_prompt("a test asset")
            .with_image_provider(&id)
            .with_3d_provider(&id)
            .with_image_model(&image_model.id)
            .with_3d_model(&model_3d.id)
            .with_output_dir(out_dir);

        let (mut progress_rx, handle, _approval_tx, _cancel_tx) = run_pipeline(config, &registry)
            .await
            .unwrap_or_else(|e| panic!("Pipeline should start for provider '{id}': {e}"));

        while progress_rx.recv().await.is_some() {}

        let output = handle
            .await
            .expect("Task should complete")
            .unwrap_or_else(|e| {
                panic!(
                    "Provider '{id}' failed in mock mode ({} / {}): {e}",
                    image_model.id, model_3d.id
                )
            });

        assert!(
            output.image_path.is_some_and(|p| p.exists()),
            "Provider '{id}' produced no image"
        );
        assert!(
            output.model_path.is_some_and(|p| p.exists()),
            "Provider '{id}' produced no 3D model"
        );

        exercised.push(id);
    }

    assert!(
        exercised.len() >= 2,
        "Expected at least fal.ai and meshy to run in mock mode, got {exercised:?}"
    );

    cleanup_mock_env();
}

// =============================================================================
// Provenance and shape
// =============================================================================

/// The key order of every object in a JSON document, in document order.
/// `serde_json::Value` sorts keys, so this reads the stream directly.
#[derive(Debug, PartialEq)]
enum Shape {
    Obj(Vec<(String, Shape)>),
    Arr(Vec<Shape>),
    Leaf,
}

impl<'de> serde::Deserialize<'de> for Shape {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Shape;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("any json")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut m: A) -> Result<Shape, A::Error> {
                let mut out = Vec::new();
                while let Some((k, v)) = m.next_entry::<String, Shape>()? {
                    out.push((k, v));
                }
                Ok(Shape::Obj(out))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut s: A) -> Result<Shape, A::Error> {
                let mut out = Vec::new();
                while let Some(v) = s.next_element::<Shape>()? {
                    out.push(v);
                }
                Ok(Shape::Arr(out))
            }
            fn visit_bool<E>(self, _: bool) -> Result<Shape, E> {
                Ok(Shape::Leaf)
            }
            fn visit_i64<E>(self, _: i64) -> Result<Shape, E> {
                Ok(Shape::Leaf)
            }
            fn visit_u64<E>(self, _: u64) -> Result<Shape, E> {
                Ok(Shape::Leaf)
            }
            fn visit_f64<E>(self, _: f64) -> Result<Shape, E> {
                Ok(Shape::Leaf)
            }
            fn visit_str<E>(self, _: &str) -> Result<Shape, E> {
                Ok(Shape::Leaf)
            }
            fn visit_unit<E>(self) -> Result<Shape, E> {
                Ok(Shape::Leaf)
            }
            fn visit_none<E>(self) -> Result<Shape, E> {
                Ok(Shape::Leaf)
            }
        }
        d.deserialize_any(V)
    }
}

/// Compare key sets and order object-by-object. `params` maps are opaque:
/// their keys are provider knobs, not schema.
fn assert_same_shape(doc: &Shape, real: &Shape, path: &str) {
    match (doc, real) {
        (Shape::Obj(a), Shape::Obj(b)) => {
            let ka: Vec<&str> = a.iter().map(|(k, _)| k.as_str()).collect();
            let kb: Vec<&str> = b.iter().map(|(k, _)| k.as_str()).collect();
            assert_eq!(ka, kb, "key set/order differs at {path}");
            for ((k, va), (_, vb)) in a.iter().zip(b) {
                if k == "params" {
                    continue;
                }
                assert_same_shape(va, vb, &format!("{path}.{k}"));
            }
        }
        (Shape::Arr(a), Shape::Arr(b)) => {
            assert_eq!(a.len(), b.len(), "array length differs at {path}");
            for (i, (va, vb)) in a.iter().zip(b).enumerate() {
                assert_same_shape(va, vb, &format!("{path}[{i}]"));
            }
        }
        (Shape::Leaf, Shape::Leaf) => {}
        (a, b) => panic!("kind differs at {path}: {a:?} vs {b:?}"),
    }
}

async fn run_mock_two_stage(temp_dir: &TempDir) -> PathBuf {
    let config = PipelineConfig::new()
        .with_prompt("a cowboy ninja")
        .with_image_model("fal-ai/nano-banana")
        .with_3d_model("fal-ai/trellis-2")
        .with_output_dir(temp_dir.path().to_path_buf());
    let registry = ProviderRegistry::new();
    let (mut rx, handle, _approval_tx, _cancel_tx) = run_pipeline(config, &registry).await.unwrap();
    while rx.recv().await.is_some() {}
    let output = handle.await.unwrap().unwrap();
    output.output_dir.unwrap().join(bundle_files::METADATA)
}

#[tokio::test]
async fn docs_bundle_example_has_the_shape_of_a_real_run() {
    let (_env, temp_dir) = setup_mock_env();
    // The embedded placeholder GLB has no textures, so the inventory is the
    // same on every machine (the on-disk demo bundle would add one). The
    // variable is `fixtures::MOCK_EMBEDDED_ENV`, private to the mock module.
    unsafe { std::env::set_var("ASSET_TAP_MOCK_EMBEDDED", "1") };
    let bundle_json = run_mock_two_stage(&temp_dir).await;
    let real = std::fs::read_to_string(&bundle_json).unwrap();
    unsafe { std::env::remove_var("ASSET_TAP_MOCK_EMBEDDED") };
    cleanup_mock_env();

    let doc = include_str!("../../docs/guides/BUNDLE_STRUCTURE.md");
    let example = doc
        .split("```json")
        .nth(1)
        .and_then(|rest| rest.split("```").next())
        .expect("doc has a json example");

    let doc_shape: Shape = serde_json::from_str(example).unwrap();
    let real_shape: Shape = serde_json::from_str(&real).unwrap();
    assert_same_shape(&doc_shape, &real_shape, "$");
}

#[tokio::test]
async fn generated_bundle_records_durations() {
    let (_env, temp_dir) = setup_mock_env();
    let bundle_json = run_mock_two_stage(&temp_dir).await;
    cleanup_mock_env();

    let metadata: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&bundle_json).unwrap()).unwrap();
    assert!(
        metadata["duration_ms"].is_u64(),
        "top-level duration: {}",
        metadata["duration_ms"]
    );
    for step in metadata["pipeline"]["steps"].as_array().unwrap() {
        assert!(
            step["duration_ms"].is_u64(),
            "step {} has no duration",
            step["id"]
        );
    }
}

#[tokio::test]
async fn supplied_image_is_recorded_as_an_import_step() {
    let (_env, temp_dir) = setup_mock_env();

    let input_dir = temp_dir.path().join("private").join("layout");
    std::fs::create_dir_all(&input_dir).unwrap();
    let test_image_path = input_dir.join("input.png");
    std::fs::write(&test_image_path, [0x89, 0x50, 0x4E, 0x47]).unwrap();

    let config = PipelineConfig::new()
        .with_existing_image(test_image_path.to_string_lossy())
        .with_3d_model("fal-ai/trellis-2")
        .with_output_dir(temp_dir.path().join("out"));
    let registry = ProviderRegistry::new();
    let (mut rx, handle, _approval_tx, _cancel_tx) = run_pipeline(config, &registry).await.unwrap();
    while rx.recv().await.is_some() {}
    let output = handle.await.unwrap().unwrap();
    cleanup_mock_env();

    let bundle_json = output.output_dir.unwrap().join(bundle_files::METADATA);
    let text = std::fs::read_to_string(&bundle_json).unwrap();
    assert!(
        !text.contains("private/layout"),
        "the input's directory must not leak into bundle.json"
    );
    let metadata: serde_json::Value = serde_json::from_str(&text).unwrap();

    let artifacts = metadata["artifacts"].as_array().unwrap();
    let image = artifacts.iter().find(|a| a["id"] == "image").unwrap();
    assert_eq!(image["produced_by"], "import");

    let steps = metadata["pipeline"]["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 2, "{steps:?}");
    assert_eq!(steps[0]["kind"], "op");
    assert_eq!(steps[0]["op"], "import");
    assert_eq!(steps[0]["params"]["source"], "input.png");
    assert_eq!(steps[0]["outputs"], serde_json::json!(["image"]));
    assert!(steps[0]["duration_ms"].is_u64());
    assert_eq!(steps[1]["modality"], "image_to_3d");
    assert_eq!(steps[1]["inputs"], serde_json::json!(["image"]));
    assert!(
        !steps.iter().any(|s| s["modality"] == "text_to_image"),
        "no image model ran"
    );
}
