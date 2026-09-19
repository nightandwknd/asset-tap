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
    // Drives the global REGISTRY, whose first touch builds a `TemplateRegistry::new()`
    // and rewrites the shared user-templates dir. Same lock instance as the
    // in-crate unit tests (see `test_support`), so they serialize.
    let _dir = asset_tap_core::test_support::templates_dir_lock();

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
    // Set fake API key so provider is available. Hold the shared env lock so
    // this doesn't race other env-mutating tests running in parallel.
    let _env = asset_tap_core::test_support::env_lock();
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
        Stage::Bind,
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
/// meshy-6-lite and meshy-7.1 are included because they share the same surface
/// minus version-specific deltas that Meshy's docs spell out per `ai_model`;
/// each delta is encoded below as a named constant so an unexplained
/// divergence still fails.
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
    let native_v6_lite = param_names(&meshy, "meshy/v6-lite/image-to-3d");
    let native_v7 = param_names(&meshy, "meshy/v7/image-to-3d");

    // Params the native Meshy API documents but fal's v6 wrapper schema
    // genuinely lacks. Verified 2026-09-18 against fal's OpenAPI for
    // `fal-ai/meshy/v6/image-to-3d`: the wrapper does not accept
    // texture_resolution, remove_lighting, or image_enhancement (it does
    // accept texture_prompt), so the gap is real, not unverified.
    //
    // decimation_mode / auto_size / origin_at are absent from fal's published
    // schema for the wrapper (checked 2026-09-19) while Meshy documents all
    // three on the native endpoint: decimation_mode "Enable adaptive
    // decimation by setting a polycount level. When set, `target_polycount`
    // is ignored."; auto_size "uses AI vision to automatically estimate the
    // real-world height of the object and resize the model accordingly";
    // origin_at "Position of the origin when `auto_size` is enabled."
    const NATIVE_ONLY: &[&str] = &[
        "texture_resolution",
        "remove_lighting",
        "image_enhancement",
        "decimation_mode",
        "auto_size",
        "origin_at",
    ];

    // No fal-only params: anything the wrapper exposes must exist natively too.
    let only_in_fal: Vec<_> = fal_v6.difference(&native_v6).collect();
    assert!(
        only_in_fal.is_empty(),
        "fal's Meshy v6 wrapper exposes parameters the native provider doesn't: {only_in_fal:?}. \
         Add them to providers/meshy.yaml so routing doesn't change the knobs."
    );

    // Everything else must match, so an unlisted addition to one copy still fails.
    let unexplained: Vec<_> = native_v6
        .difference(&fal_v6)
        .filter(|name| !NATIVE_ONLY.contains(&name.as_str()))
        .collect();
    assert!(
        unexplained.is_empty(),
        "Native Meshy v6 gained parameters that fal's wrapper lacks: {unexplained:?}. \
         Either add them to fal-ai.yaml's meshy/v6 block or, if fal's wrapper \
         genuinely doesn't support them, add them to NATIVE_ONLY with a note."
    );

    // Meshy 6 Lite replaced the retired meshy-5 (Meshy: successor to meshy-5,
    // same parameters, same credit cost), so it inherits v5's expectation:
    // v6's surface minus the knobs Meshy gates away from it.
    //   - texture_resolution: not model-gated as such, but "The 4k and 8k
    //     options are unavailable with meshy-6-lite" leaves a 2k-only
    //     dropdown, so the knob is omitted rather than shipped inert.
    //   - image_enhancement: "only available with meshy-6, meshy-7.1, or latest".
    //   - remove_lighting: "supported only with meshy-6".
    const V6_ONLY: &[&str] = &["texture_resolution", "remove_lighting", "image_enhancement"];

    let expected_v6_lite: BTreeSet<String> = native_v6
        .iter()
        .filter(|name| !V6_ONLY.contains(&name.as_str()))
        .cloned()
        .collect();
    assert_eq!(
        native_v6_lite,
        expected_v6_lite,
        "Meshy 6 Lite should expose v6's surface minus the documented v6-only knobs \
         ({V6_ONLY:?}). Mismatch: only-in-v6-lite={:?}, missing-from-v6-lite={:?}",
        native_v6_lite
            .difference(&expected_v6_lite)
            .collect::<Vec<_>>(),
        expected_v6_lite
            .difference(&native_v6_lite)
            .collect::<Vec<_>>()
    );

    // Meshy v7 (served natively and, since 2026-08, via fal's partner-
    // namespace wrapper `meshy/v7/image-to-3d`) is v6's surface plus/minus
    // the documented per-version deltas:
    //   - remove_lighting: "supported only with meshy-6".
    //   + geometry_resolution: "Requires meshy-7.1 or latest". It supersedes
    //     ultra_mode, which Meshy deprecated ("ultra_mode: true is equivalent
    //     to geometry_resolution: '2k'"), so no model advertises ultra_mode.
    // symmetry_mode is NOT listed here: Meshy deprecated it API-wide ("This
    // parameter no longer affects output"), so it is no longer a declared
    // parameter on any model — it only rides along inert in the shared
    // request-body anchor. Nothing to subtract.
    const NOT_IN_V7: &[&str] = &["remove_lighting"];
    const V7_ONLY: &[&str] = &["geometry_resolution"];

    let expected_v7: BTreeSet<String> = native_v6
        .iter()
        .filter(|name| !NOT_IN_V7.contains(&name.as_str()))
        .cloned()
        .chain(V7_ONLY.iter().map(|s| s.to_string()))
        .collect();
    assert_eq!(
        native_v7,
        expected_v7,
        "Meshy v7 should expose v6's surface minus {NOT_IN_V7:?} plus {V7_ONLY:?}. \
         Mismatch: only-in-v7={:?}, missing-from-v7={:?}",
        native_v7.difference(&expected_v7).collect::<Vec<_>>(),
        expected_v7.difference(&native_v7).collect::<Vec<_>>()
    );

    // fal's v7 wrapper mirrors the native v7 surface, except two params fal's
    // published schema genuinely lacks (unlike v6, texture_prompt DOES pass
    // through on v7 — confirmed against fal's OpenAPI for
    // `meshy/v7/image-to-3d`, 2026-09-18).
    //
    // geometry_resolution is on BOTH, with different option lists: fal's enum
    // is ["standard", "2k"] ("Meshy-7 supports standard and 2k") where the
    // native API also documents "4k". This test compares parameter NAMES
    // only, deliberately — each provider must advertise exactly the values
    // its own schema accepts, so option lists are allowed to diverge where
    // the schemas do. `meshy_v7_geometry_resolution_options_follow_each_schema`
    // below pins those two lists so neither drifts unnoticed.
    // decimation_mode / auto_size / origin_at are missing from fal's v7
    // schema for the same reason as v6 — the wrapper predates them.
    const V7_NATIVE_ONLY: &[&str] = &[
        "texture_resolution",
        "image_enhancement",
        "decimation_mode",
        "auto_size",
        "origin_at",
    ];

    let fal_v7 = param_names(&fal, "fal-ai/meshy/v7/image-to-3d");

    let only_in_fal_v7: Vec<_> = fal_v7.difference(&native_v7).collect();
    assert!(
        only_in_fal_v7.is_empty(),
        "fal's Meshy v7 wrapper exposes parameters the native provider doesn't: {only_in_fal_v7:?}. \
         Add them to providers/meshy.yaml so routing doesn't change the knobs."
    );

    let unexplained_v7: Vec<_> = native_v7
        .difference(&fal_v7)
        .filter(|name| !V7_NATIVE_ONLY.contains(&name.as_str()))
        .collect();
    assert!(
        unexplained_v7.is_empty(),
        "Native Meshy v7 gained parameters that fal's wrapper lacks: {unexplained_v7:?}. \
         Either add them to fal-ai.yaml's meshy v7 block or, if fal's wrapper \
         genuinely doesn't support them, add them to V7_NATIVE_ONLY with a note."
    );

    // Smart Topology (meshy-t2) has its own surface: Meshy documents
    // topology / should_remesh (and save_pre_remeshed_model) as IGNORED for
    // smart-topology tasks — advertising them would offer dead knobs. Pin the exact surface
    // so an accidental copy-paste from the v5/v6/v7 lists fails loudly.
    let native_t2 = param_names(&meshy, "meshy/t2/image-to-3d");
    let expected_t2: BTreeSet<String> = [
        "target_polycount",
        "should_texture",
        "enable_pbr",
        "pose_mode",
        // auto_size/origin_at are not remesh-gated, so Smart Topology gets
        // them too; decimation_mode is, and t2 ignores should_remesh.
        "auto_size",
        "origin_at",
        "texture_prompt",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(
        native_t2, expected_t2,
        "Smart Topology's surface changed. If Meshy's docs added a knob, extend \
         the expected list; never advertise topology/should_remesh \
         (ignored for smart-topology)."
    );
}

/// `meshy_v6_parameter_surface_matches_across_providers` compares parameter
/// NAMES, which is right: each provider must advertise exactly the values its
/// own schema accepts. geometry_resolution is the case where those diverge —
/// Meshy's native API documents standard/2k/4k ("2k runs the Ultra pass at
/// 2048³; 4k at 4096³"), while fal's wrapper enum is ["standard", "2k"]
/// ("Geometry resolution. Meshy-7 supports standard and 2k"). Pin both so the
/// divergence stays the verified one and neither list drifts silently.
#[test]
fn meshy_v7_geometry_resolution_options_follow_each_schema() {
    use asset_tap_core::providers::config::ProviderConfig;

    fn options(yaml_path: &str, model_id: &str, param: &str) -> Vec<String> {
        let full =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../{}", yaml_path));
        let config = ProviderConfig::from_yaml_file(&full)
            .unwrap_or_else(|e| panic!("loading {}: {}", full.display(), e));
        config
            .image_to_3d
            .iter()
            .find(|m| m.id == model_id)
            .unwrap_or_else(|| panic!("model {} not found", model_id))
            .parameters
            .iter()
            .find(|p| p.name == param)
            .unwrap_or_else(|| panic!("{} has no {} parameter", model_id, param))
            .options
            .clone()
            .unwrap_or_default()
            .iter()
            .map(|v| v.as_str().unwrap_or_default().to_string())
            .collect()
    }

    assert_eq!(
        options(
            "providers/meshy.yaml",
            "meshy/v7/image-to-3d",
            "geometry_resolution"
        ),
        vec!["standard", "2k", "4k"],
        "Native Meshy documents geometry_resolution standard/2k/4k on meshy-7.1."
    );
    assert_eq!(
        options(
            "providers/fal-ai.yaml",
            "fal-ai/meshy/v7/image-to-3d",
            "geometry_resolution"
        ),
        vec!["standard", "2k"],
        "fal's v7 schema enum is [standard, 2k] — do not copy the native 4k in \
         until fal's OpenAPI advertises it."
    );
}

/// The nano-banana family shares one YAML anchor for its text-to-image
/// parameters, but gpt-image-2 duplicates the list because its aspect-ratio
/// options differ. That copy is drift-prone: a param added to the anchor
/// (e.g. remove_background, 2026-08) must be hand-added to gpt-image-2 too.
/// This test pins the param NAMES equal across all four models — only the
/// option values may differ.
#[test]
fn meshy_text_to_image_surface_matches_across_models() {
    use asset_tap_core::providers::config::ProviderConfig;
    use std::collections::BTreeSet;

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../providers/meshy.yaml");
    let config = ProviderConfig::from_yaml_file(&path).expect("meshy.yaml loads");

    let surfaces: Vec<(String, BTreeSet<String>)> = config
        .text_to_image
        .iter()
        .map(|m| {
            (
                m.id.clone(),
                m.parameters.iter().map(|p| p.name.clone()).collect(),
            )
        })
        .collect();
    assert!(surfaces.len() >= 4, "expected at least 4 Meshy t2i models");

    let (first_id, first) = &surfaces[0];
    for (id, names) in &surfaces[1..] {
        assert_eq!(
            names, first,
            "Meshy t2i models must expose the same parameter names; {id} differs \
             from {first_id}. gpt-image-2 duplicates the shared anchor — keep the \
             copies in sync (only aspect_ratio's OPTIONS may differ)."
        );
    }
}

/// Meshy rejects a request that sets `aspect_ratio` while `generate_multi_view`
/// is true — the two are mutually exclusive, not merely redundant.
///
/// The schema says so directly now: `conflicts_with` on aspect_ratio makes
/// turning on Multi-View drop the aspect ratio automatically, and setting both
/// explicitly a usage error. `allow_unset` stays alongside it — the conflict
/// handles Multi-View, while allow_unset is how a user asks for Meshy's own
/// default with Multi-View off. Any model offering both knobs must declare
/// both.
#[test]
fn mutually_exclusive_select_params_are_clearable() {
    use asset_tap_core::providers::config::ProviderConfig;

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../providers/meshy.yaml");
    let config = ProviderConfig::from_yaml_file(&path).expect("meshy.yaml loads");

    let mut checked = 0;
    for model in &config.text_to_image {
        let has_multi_view = model
            .parameters
            .iter()
            .any(|p| p.name == "generate_multi_view");
        if !has_multi_view {
            continue;
        }
        let aspect = model
            .parameters
            .iter()
            .find(|p| p.name == "aspect_ratio")
            .unwrap_or_else(|| {
                panic!(
                    "{}: declares generate_multi_view but no aspect_ratio",
                    model.id
                )
            });
        assert_eq!(
            aspect.conflicts_with.get("generate_multi_view"),
            Some(&serde_json::json!(true)),
            "{}: aspect_ratio must declare `conflicts_with: {{ generate_multi_view: true }}` — \
             Meshy rejects the pair, so the GUI has to grey it out and `--param` has to refuse it",
            model.id
        );
        assert!(
            aspect.allow_unset,
            "{}: aspect_ratio must keep `allow_unset: true` — the conflict covers Multi-View, \
             but a GUI dropdown still needs an entry for \"leave it to Meshy\"",
            model.id
        );
        checked += 1;
    }

    assert!(
        checked >= 4,
        "expected every Meshy text-to-image model to be checked, saw {checked}"
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

/// Every declared parameter's `default:` value must match the corresponding
/// `request.body` template value. They drift when someone updates one but
/// not the other — the body controls what gets sent to the API, the parameter
/// declaration controls what gets recorded in `bundle.json` (via
/// `merge_param_overrides`). When they disagree, the bundle no longer
/// reproduces what was actually sent.
///
/// Skips body values that are interpolated strings (`${...}`) since those
/// have no static default to compare against.
#[test]
fn parameter_defaults_match_request_body_templates() {
    use asset_tap_core::providers::config::{ModelConfig, ProviderConfig};

    fn load(yaml_path: &str) -> ProviderConfig {
        let full =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../{}", yaml_path));
        ProviderConfig::from_yaml_file(&full)
            .unwrap_or_else(|e| panic!("loading {}: {}", full.display(), e))
    }

    /// Two JSON values are "default-equivalent" if they encode the same
    /// scalar after numeric normalization — `1` and `1.0` should match.
    fn equivalent(a: &serde_json::Value, b: &serde_json::Value) -> bool {
        match (a, b) {
            (serde_json::Value::Number(x), serde_json::Value::Number(y)) => {
                match (x.as_f64(), y.as_f64()) {
                    (Some(xf), Some(yf)) => xf == yf,
                    _ => x == y,
                }
            }
            _ => a == b,
        }
    }

    fn check_model(provider_id: &str, model: &ModelConfig, drift: &mut Vec<String>) {
        let Some(body) = model.request.body.as_ref().and_then(|v| v.as_object()) else {
            return;
        };
        for param in &model.parameters {
            let Some(body_val) = body.get(&param.name) else {
                continue; // missing-key bug is caught by the sibling test
            };
            // Interpolated templates ('${prompt}', etc.) have no static default.
            if matches!(body_val, serde_json::Value::String(s) if s.contains("${")) {
                continue;
            }
            if !equivalent(body_val, &param.default) {
                drift.push(format!(
                    "{} / {}: parameter '{}' default {} differs from request.body template {}",
                    provider_id, model.id, param.name, param.default, body_val
                ));
            }
        }
    }

    let mut drift = Vec::new();
    for path in ["providers/fal-ai.yaml", "providers/meshy.yaml"] {
        let config = load(path);
        let provider_id = config.provider.id.clone();
        for model in config.text_to_image.iter().chain(config.image_to_3d.iter()) {
            check_model(&provider_id, model, &mut drift);
        }
    }

    assert!(
        drift.is_empty(),
        "Parameter defaults drifted from request body templates (the body controls what's sent; the declaration controls what's recorded in bundle.json):\n  {}",
        drift.join("\n  ")
    );
}
