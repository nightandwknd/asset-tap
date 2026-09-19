//! `--dump-provider-config` — the tooling hook that lets auditing scripts read
//! the app's own parse of a provider YAML (anchors resolved by serde_yaml)
//! instead of re-implementing a YAML reader.
//!
//! scripts/audit-fal-schemas.{sh,py} depends on this shape: a JSON object with
//! `text_to_image` / `image_to_3d` arrays whose entries carry `id`,
//! `request.body` and `parameters`.

use serde_json::Value;
use std::process::Command;

const PROVIDER_ID: &str = "fal.ai";

fn dump(provider_id: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_asset-tap"))
        .arg("--dump-provider-config")
        .arg(provider_id)
        .output()
        .expect("failed to run asset-tap")
}

#[test]
fn dump_provider_config_emits_parseable_model_configs() {
    let out = dump(PROVIDER_ID);
    assert!(
        out.status.success(),
        "exit {:?}, stderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );

    let doc: Value = serde_json::from_slice(&out.stdout).expect("stdout should be JSON");

    let models: Vec<&Value> = ["text_to_image", "image_to_3d"]
        .iter()
        .flat_map(|stage| doc[stage].as_array().expect("stage array"))
        .collect();

    // fal.ai ships 5 text-to-image and 4 image-to-3D models; the audit script
    // fetches one OpenAPI schema per id, so a silent drop here would silently
    // shrink its coverage.
    assert_eq!(models.len(), 9, "expected 9 fal.ai models, got {models:?}");

    for model in models {
        let id = model["id"].as_str().expect("model id");
        assert!(
            model["request"]["body"].is_object(),
            "{id}: request.body should be an object"
        );
        let params = model["parameters"]
            .as_array()
            .unwrap_or_else(|| panic!("{id}: parameters should be an array"));
        assert!(!params.is_empty(), "{id}: expected tunable parameters");
        for p in params {
            assert!(p["name"].is_string(), "{id}: parameter missing a name");
        }
    }
}

/// An unknown id is a usage error: exit 2, before anything is written to
/// stdout (see docs/CLI_MACHINE_INTERFACE.md §2).
#[test]
fn dump_provider_config_rejects_unknown_provider() {
    let out = dump("not-a-provider");
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty(), "stdout should stay clean");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("not-a-provider"),
        "stderr should name the bad id"
    );
}
