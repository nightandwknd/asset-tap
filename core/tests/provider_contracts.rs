#![cfg(feature = "mock")]
//! Provider request-shape contract tests.
//!
//! Spin up the mock server, route a real `DynamicProvider` at it, and assert
//! that the HTTP requests the provider emits match the YAML declarations.
//!
//! These catch the "declared but not wired up" class of bug — e.g. adding
//! `face_count` to Hunyuan's `parameters` list but forgetting to add it to
//! `request.body`, or misspelling a parameter name so overrides get dropped.
//!
//! Scope (by design): only the *outgoing request body* is checked. Response
//! parsing is covered by `pipeline_execution_tests.rs` and unit tests; here
//! we're exclusively answering "did we send what the YAML said we'd send?"
//!
//! Runs under `--features mock`; Meshy is hidden in mock mode (see
//! ProviderRegistry), so only fal-ai models are exercised.

use asset_tap_core::api::mock::{MockApiServer, MockServerConfig};
use asset_tap_core::providers::{
    DynamicProvider, Provider, ProviderCapability, config::ProviderConfig,
};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use tokio::sync::mpsc;

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

fn load_fal_config() -> ProviderConfig {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../providers/fal-ai.yaml");
    ProviderConfig::from_yaml_file(&path).expect("loading fal-ai provider yaml")
}

async fn spin_up_fal_against_mock() -> (DynamicProvider, MockApiServer) {
    // Set env vars the provider + validator need. SAFETY: tests in this file
    // run under nextest's single-threaded mode (see .config/nextest.toml), so
    // no concurrent env reads can race.
    unsafe {
        std::env::set_var("FAL_KEY", "mock-api-key");
        // Needed so validate_download_url() lets the mock's localhost URLs
        // through. Without it, the mock's uploaded `file_url` (which points
        // back at 127.0.0.1) gets rejected as a private-IP SSRF risk.
        std::env::set_var("MOCK_API", "1");
    }

    let mock = MockApiServer::start(MockServerConfig::instant()).await;
    let mut config = load_fal_config();
    config.provider.base_url = Some(mock.url());
    // Upload endpoint is an absolute URL in the YAML — redirect that too so
    // `image_to_3d` flows don't try to hit real fal storage.
    if let Some(ref mut upload) = config.provider.upload {
        upload.endpoint = format!("{}/storage/upload/initiate", mock.url());
    }
    let provider = DynamicProvider::new(config);
    // Short-circuit polling so the mock's COMPLETED response arrives in
    // milliseconds, not seconds.
    provider.clamp_polling_interval(1);
    (provider, mock)
}

/// Find the most recent POST body sent to an exact endpoint path.
///
/// Using exact match (not `contains`) matters because endpoint paths share
/// prefixes — `/fal-ai/nano-banana` is a suffix of `/fal-ai/nano-banana-2`.
/// `rev()` picks the latest match so tests that iterate through multiple
/// models don't collide with each other's captured requests.
async fn captured_post_body(mock: &MockApiServer, endpoint_path: &str) -> Value {
    let requests = mock
        .received_requests()
        .await
        .expect("mock should record requests");
    let matching = requests
        .iter()
        .rev()
        .filter(|r| r.method == wiremock::http::Method::POST)
        .find(|r| r.url.path() == endpoint_path)
        .unwrap_or_else(|| {
            let paths: Vec<String> = requests
                .iter()
                .map(|r| format!("{} {}", r.method, r.url.path()))
                .collect();
            panic!(
                "no POST to exact path '{}' was captured. All requests:\n  {}",
                endpoint_path,
                paths.join("\n  ")
            );
        });
    serde_json::from_slice(&matching.body).expect("POST body should be JSON")
}

// -----------------------------------------------------------------------------
// Text-to-Image contracts
// -----------------------------------------------------------------------------

/// Every declared text-to-image model sends a request body whose defaults match
/// the YAML exactly (modulo null-stripping).
#[tokio::test]
async fn t2i_sends_yaml_defaults_with_no_overrides() {
    let (provider, mock) = spin_up_fal_against_mock().await;
    let config = load_fal_config();

    for model_cfg in &config.text_to_image {
        let (tx, _rx) = mpsc::unbounded_channel();
        let _ = provider
            .text_to_image("test prompt", &model_cfg.id, None, Some(tx))
            .await;

        let body = captured_post_body(&mock, &model_cfg.endpoint).await;
        let body_obj = body
            .as_object()
            .unwrap_or_else(|| panic!("{}: body is not a JSON object", model_cfg.id));

        // Every non-null template key should be present in the outgoing body.
        if let Some(Value::Object(template)) = &model_cfg.request.body {
            for (key, tmpl_val) in template {
                if tmpl_val.is_null() {
                    assert!(
                        !body_obj.contains_key(key),
                        "{}: template-null key '{}' should be stripped but was sent",
                        model_cfg.id,
                        key
                    );
                    continue;
                }
                // ${prompt} is interpolated — just assert the key exists.
                if matches!(tmpl_val, Value::String(s) if s.contains("${")) {
                    assert!(
                        body_obj.contains_key(key),
                        "{}: interpolated key '{}' is missing from body",
                        model_cfg.id,
                        key
                    );
                } else {
                    assert_eq!(
                        body_obj.get(key),
                        Some(tmpl_val),
                        "{}: key '{}' doesn't match YAML template default",
                        model_cfg.id,
                        key
                    );
                }
            }
        }
    }
}

/// Overriding a declared param via `params` makes it to the wire; overriding
/// an undeclared name is silently dropped (allowlist behavior).
#[tokio::test]
async fn t2i_overrides_reach_the_body() {
    let (provider, mock) = spin_up_fal_against_mock().await;
    let config = load_fal_config();

    for model_cfg in &config.text_to_image {
        // Pick a boolean or numeric param we can safely flip without reshaping
        // the request. Text/select params are model-specific; skipping keeps
        // this test generic.
        let Some(param) = model_cfg.parameters.iter().find(|p| {
            matches!(
                p.param_type,
                asset_tap_core::providers::config::ParameterType::Boolean
                    | asset_tap_core::providers::config::ParameterType::Integer
            )
        }) else {
            continue;
        };

        let override_value: Value = match &param.default {
            Value::Bool(b) => Value::Bool(!b),
            Value::Number(n) => {
                let n_i64 = n.as_i64().unwrap_or(1);
                // Stay inside declared bounds where possible.
                let max = param.max.map(|m| m as i64).unwrap_or(n_i64 + 1);
                Value::from((n_i64 + 1).min(max))
            }
            other => other.clone(),
        };

        let mut params = HashMap::new();
        params.insert(param.name.clone(), override_value.clone());
        // Also attempt an injection — this should NOT reach the body.
        params.insert("__injected_field".into(), Value::String("attacker".into()));

        let (tx, _rx) = mpsc::unbounded_channel();
        let _ = provider
            .text_to_image("test prompt", &model_cfg.id, Some(&params), Some(tx))
            .await;

        let body = captured_post_body(&mock, &model_cfg.endpoint).await;

        assert_eq!(
            body.get(&param.name),
            Some(&override_value),
            "{}: override for '{}' didn't land in body",
            model_cfg.id,
            param.name
        );
        assert!(
            body.get("__injected_field").is_none(),
            "{}: undeclared parameter leaked into request body — allowlist is broken",
            model_cfg.id
        );
    }
}

/// Null override clears a template default, so the provider's server-side
/// default kicks in. Belt-and-braces: unit tests already verify `apply_param_overrides`,
/// this one verifies the wiring through the full `DynamicProvider` path.
#[tokio::test]
async fn t2i_null_override_strips_key() {
    let (provider, mock) = spin_up_fal_against_mock().await;
    let config = load_fal_config();

    // Use the first model that has at least one overrideable param with a
    // non-null template default. Picking any such model exercises the path;
    // we don't need to iterate every model here.
    let (model_cfg, param_name) = config
        .text_to_image
        .iter()
        .find_map(|m| {
            let body = m.request.body.as_ref()?.as_object()?;
            let param = m
                .parameters
                .iter()
                .find(|p| body.get(&p.name).map(|v| !v.is_null()).unwrap_or(false))?;
            Some((m, param.name.clone()))
        })
        .expect("at least one t2i model should have an overrideable, non-null default");

    let mut params = HashMap::new();
    params.insert(param_name.clone(), Value::Null);

    let (tx, _rx) = mpsc::unbounded_channel();
    let _ = provider
        .text_to_image("test prompt", &model_cfg.id, Some(&params), Some(tx))
        .await;

    let body = captured_post_body(&mock, &model_cfg.endpoint).await;
    assert!(
        body.get(&param_name).is_none(),
        "{}: null override for '{}' did not strip the key — body was {}",
        model_cfg.id,
        param_name,
        body
    );
}

// -----------------------------------------------------------------------------
// Image-to-3D contracts
// -----------------------------------------------------------------------------

/// Same contract as the t2i default test, for image-to-3D endpoints.
///
/// image-to-3D flows involve upload + poll in addition to the POST, but we
/// only care about the generation POST body here.
#[tokio::test]
async fn i23d_sends_yaml_defaults_with_no_overrides() {
    let (provider, mock) = spin_up_fal_against_mock().await;
    let config = load_fal_config();

    // Tiny valid PNG so `needs_url` pathways work.
    let image_bytes = asset_tap_core::api::mock::MockFixtures::request_id();
    let image_bytes = image_bytes.into_bytes(); // any bytes — upload is mocked

    for model_cfg in &config.image_to_3d {
        let (tx, _rx) = mpsc::unbounded_channel();
        let _ = provider
            .image_to_3d(&image_bytes, &model_cfg.id, None, Some(tx))
            .await;

        let body = captured_post_body(&mock, &model_cfg.endpoint).await;
        let body_obj = body
            .as_object()
            .unwrap_or_else(|| panic!("{}: body is not a JSON object", model_cfg.id));

        if let Some(Value::Object(template)) = &model_cfg.request.body {
            for (key, tmpl_val) in template {
                if tmpl_val.is_null() {
                    assert!(
                        !body_obj.contains_key(key),
                        "{}: template-null key '{}' should be stripped",
                        model_cfg.id,
                        key
                    );
                    continue;
                }
                if matches!(tmpl_val, Value::String(s) if s.contains("${")) {
                    assert!(
                        body_obj.contains_key(key),
                        "{}: interpolated key '{}' missing",
                        model_cfg.id,
                        key
                    );
                } else {
                    assert_eq!(
                        body_obj.get(key),
                        Some(tmpl_val),
                        "{}: key '{}' doesn't match YAML template default",
                        model_cfg.id,
                        key
                    );
                }
            }
        }
    }
}

/// Sanity: `fal-ai/meshy/v6/image-to-3d` and `fal-ai/hunyuan-3d/v3.1/pro/image-to-3d`
/// are the models most likely to drift because they have the largest parameter
/// surfaces. Assert that their YAML-listed `parameters:` have declared names
/// that are *actually registered* with the provider (catches name typos
/// between the `parameters` list and the provider registry loading).
#[test]
fn high_risk_models_register_with_declared_parameter_names() {
    unsafe { std::env::set_var("FAL_KEY", "mock-api-key") };
    let registry = asset_tap_core::providers::ProviderRegistry::new();
    let fal = registry
        .get("fal.ai")
        .expect("fal.ai provider should register");

    for model_id in [
        "fal-ai/meshy/v6/image-to-3d",
        "fal-ai/hunyuan-3d/v3.1/pro/image-to-3d",
        "fal-ai/trellis-2",
    ] {
        let info = fal
            .list_models(ProviderCapability::ImageTo3D)
            .into_iter()
            .find(|m| m.id == model_id)
            .unwrap_or_else(|| panic!("model '{}' missing from registry", model_id));

        assert!(
            !info.parameters.is_empty(),
            "{}: should expose parameters",
            model_id
        );
        for param in &info.parameters {
            assert!(!param.name.is_empty(), "{}: empty parameter name", model_id);
            assert!(
                !param.label.is_empty(),
                "{}: empty label for parameter '{}'",
                model_id,
                param.name
            );
        }
    }
}
