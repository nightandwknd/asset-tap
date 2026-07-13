//! Machine-interface (`--json`) wire-format tests.
//!
//! These run against the golden fixtures in `tests/fixtures/machine-interface/`
//! — the same bytes vendored by downstream consumers. Together they are the drift
//! alarm described in docs/CLI_MACHINE_INTERFACE.md §6: if the wire format
//! changes here without the fixtures being regenerated (and re-vendored),
//! these tests fail.

use asset_tap::machine::{self, Event};
use asset_tap_core::providers::{ParameterDef, ParameterType, ParameterWidget};
use asset_tap_core::types::{
    ApiError, ApiErrorKind, ApiProvider, Error as CoreError, Progress, Stage,
};
use serde_json::Value;

const FIXTURE_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/machine-interface"
);

fn read_fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{FIXTURE_DIR}/{name}"))
        .unwrap_or_else(|e| panic!("failed to read fixture {name}: {e}"))
}

/// Every NDJSON golden fixture. New fixtures must be added here (once) to be
/// covered by all stream-contract tests below.
const NDJSON_FIXTURES: &[&str] = &[
    "success.ndjson",
    "provider_error.ndjson",
    "rate_limited_retry.ndjson",
    "canceled.ndjson",
];

/// Every non-empty line of an NDJSON fixture must parse as a JSON object with a
/// string `event` field — the fundamental consumer guarantee (spec §1).
#[test]
fn all_ndjson_fixtures_parse_as_event_objects() {
    for &name in NDJSON_FIXTURES {
        let content = read_fixture(name);
        for (i, line) in content.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("{name}:{} is not valid JSON: {e}", i + 1));
            assert!(
                value.get("event").and_then(Value::as_str).is_some(),
                "{name}:{} missing string `event` field",
                i + 1
            );
        }
    }
}

/// Structural invariant: `start` is the first line and `result` the last,
/// exactly one of each (spec §6 acceptance checklist).
#[test]
fn fixtures_start_first_result_last() {
    for &name in NDJSON_FIXTURES {
        let content = read_fixture(name);
        let events: Vec<Value> = content
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();

        let kinds: Vec<&str> = events
            .iter()
            .map(|e| e["event"].as_str().unwrap())
            .collect();

        assert_eq!(
            kinds.first(),
            Some(&"start"),
            "{name}: first line not start"
        );
        assert_eq!(
            kinds.last(),
            Some(&"result"),
            "{name}: last line not result"
        );
        assert_eq!(
            kinds.iter().filter(|k| **k == "start").count(),
            1,
            "{name}: expected exactly one start"
        );
        assert_eq!(
            kinds.iter().filter(|k| **k == "result").count(),
            1,
            "{name}: expected exactly one result"
        );
    }
}

/// The `start` event declares `interface: "1.0"` matching the module constant.
#[test]
fn start_event_declares_interface_version() {
    let content = read_fixture("success.ndjson");
    let first: Value = serde_json::from_str(content.lines().next().unwrap()).unwrap();
    assert_eq!(
        first["interface"].as_str(),
        Some(machine::INTERFACE_VERSION)
    );
}

/// Each NDJSON fixture line is compact single-line JSON (no embedded newlines,
/// no leading/trailing whitespace) — the shape `emit()` writes. Field *order*
/// is validated separately by driving the real `Event` types
/// (`machine_events_match_fixture_lines`); a generic `Value` re-serialize can't
/// check order because it doesn't preserve key order.
#[test]
fn ndjson_fixtures_are_single_line_json() {
    for &name in NDJSON_FIXTURES {
        let content = read_fixture(name);
        for (i, line) in content.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            assert_eq!(
                line,
                line.trim(),
                "{name}:{} has surrounding whitespace",
                i + 1
            );
            // Valid JSON, and one object per physical line.
            let _: Value = serde_json::from_str(line).unwrap();
        }
    }
}

/// Events built by the `machine` module serialize to exactly the bytes in the
/// fixtures. This is the drift check that couples production code to the golden
/// files: change a field name or the emit format and these stop matching.
#[test]
fn machine_events_match_fixture_lines() {
    // start
    let start = serde_json::to_value(Event::start()).unwrap();
    assert_eq!(start["event"], "start");
    assert_eq!(
        start["interface"].as_str(),
        Some(machine::INTERFACE_VERSION)
    );

    // A representative selection of progress events, compared against the exact
    // fixture lines they'd produce.
    let cases: Vec<(Progress, &str)> = vec![
        (
            Progress::Started {
                stage: Stage::ImageGeneration,
            },
            r#"{"event":"progress","stage":"image_generation","state":"started"}"#,
        ),
        (
            Progress::Queued {
                stage: Stage::ImageGeneration,
                position: 3,
            },
            r#"{"event":"progress","stage":"image_generation","state":"queued","position":3}"#,
        ),
        (
            Progress::Downloading {
                stage: Stage::Download,
                bytes_downloaded: 1048576,
                total_bytes: Some(36076232),
            },
            r#"{"event":"progress","stage":"download","state":"downloading","bytes_downloaded":1048576,"total_bytes":36076232}"#,
        ),
        (
            Progress::Retrying {
                stage: Stage::Model3DGeneration,
                attempt: 2,
                max_attempts: 5,
                delay_secs: 10,
                reason: "rate limited".to_string(),
            },
            r#"{"event":"progress","stage":"model_3d_generation","state":"retrying","attempt":2,"max_attempts":5,"delay_secs":10,"reason":"rate limited"}"#,
        ),
    ];
    for (progress, expected) in cases {
        let ev = machine::progress_event(&progress).unwrap();
        let line = serde_json::to_string(&ev).unwrap();
        assert_eq!(line, expected, "event bytes drifted from spec/fixture");
    }

    // The canceled result matches the canceled fixture's last line.
    let canceled =
        serde_json::to_string(&Event::result_canceled(Some(Stage::Model3DGeneration))).unwrap();
    assert_eq!(
        canceled,
        r#"{"event":"result","status":"canceled","stage":"model_3d_generation"}"#
    );

    // Every line asserted above appears verbatim in a fixture — belt and
    // suspenders against the fixture and the code diverging.
    let all_fixture_lines: String = NDJSON_FIXTURES.iter().map(|f| read_fixture(f)).collect();
    for line in [
        r#"{"event":"progress","stage":"image_generation","state":"started"}"#,
        r#"{"event":"progress","stage":"image_generation","state":"queued","position":3}"#,
        r#"{"event":"progress","stage":"download","state":"downloading","bytes_downloaded":1048576,"total_bytes":36076232}"#,
        r#"{"event":"progress","stage":"model_3d_generation","state":"retrying","attempt":2,"max_attempts":5,"delay_secs":10,"reason":"rate limited"}"#,
        r#"{"event":"result","status":"canceled","stage":"model_3d_generation"}"#,
    ] {
        assert!(
            all_fixture_lines.contains(line),
            "asserted event line not present in any fixture: {line}"
        );
    }
}

/// The catalog fixture is exactly what `print_catalog` writes for a `Catalog`
/// carrying the fixture's data. We build the `Catalog` in code (the production
/// type is serialize-only — consumers only read it) and assert the pretty-printed
/// bytes match. This couples the fixture to the `Catalog` field order and
/// serde attributes: reorder a field or rename one and this fails.
#[test]
fn catalog_fixture_matches_catalog_serialization() {
    use machine::{
        Catalog, CatalogModel, CatalogParameter, CatalogProvider, CatalogTemplate,
        CatalogTemplateVariable,
    };

    let catalog = Catalog {
        interface: machine::INTERFACE_VERSION,
        providers: vec![CatalogProvider {
            id: "fal.ai".to_string(),
            name: "fal.ai".to_string(),
            description: "Serverless AI model APIs".to_string(),
            configured: true,
            required_env_vars: vec!["FAL_KEY".to_string()],
            models: vec![
                CatalogModel {
                    id: "fal-ai/flux-2".to_string(),
                    name: "FLUX.2".to_string(),
                    description: Some("High-quality text-to-image model".to_string()),
                    modality: "text_to_image",
                    is_default: true,
                    parameters: vec![CatalogParameter {
                        name: "guidance_scale".to_string(),
                        label: "Guidance Scale".to_string(),
                        description: Some("Higher = stricter prompt adherence".to_string()),
                        param_type: "float",
                        default: serde_json::json!(3.5),
                        min: Some(1.0),
                        max: Some(20.0),
                        step: Some(0.5),
                        options: None,
                        widget: None,
                    }],
                },
                CatalogModel {
                    id: "fal-ai/trellis-2".to_string(),
                    name: "Trellis 2".to_string(),
                    description: Some("Image-to-3D mesh generation".to_string()),
                    modality: "image_to_3d",
                    is_default: true,
                    parameters: vec![
                        CatalogParameter {
                            name: "resolution".to_string(),
                            label: "Resolution".to_string(),
                            description: None,
                            param_type: "select",
                            default: serde_json::json!(1024),
                            min: None,
                            max: None,
                            step: None,
                            options: Some(vec![
                                serde_json::json!(512),
                                serde_json::json!(1024),
                                serde_json::json!(1536),
                            ]),
                            widget: None,
                        },
                        CatalogParameter {
                            name: "seed".to_string(),
                            label: "Seed".to_string(),
                            description: Some("Leave blank for random".to_string()),
                            param_type: "integer",
                            default: serde_json::Value::Null,
                            min: Some(0.0),
                            max: Some(2147483647.0),
                            step: None,
                            options: None,
                            widget: Some("input"),
                        },
                    ],
                },
            ],
        }],
        templates: Some(vec![CatalogTemplate {
            id: "humanoid".to_string(),
            name: "Humanoid Character".to_string(),
            description: "A stylized humanoid character, T-pose, game-ready".to_string(),
            category: Some("character".to_string()),
            variables: vec![CatalogTemplateVariable {
                name: "description".to_string(),
                description: Some("What the character looks like".to_string()),
                required: true,
            }],
            examples: vec!["a cowboy ninja with dual katanas".to_string()],
        }]),
    };

    let serialized = serde_json::to_string_pretty(&catalog).unwrap();
    let fixture = read_fixture("catalog.json");
    assert_eq!(
        serialized.trim_end(),
        fixture.trim_end(),
        "catalog.json drifted from Catalog serialization — regenerate the fixture \
         and re-vendor it in consumers"
    );
}

/// Stage wire names match the spec exactly. Model3DGeneration is the trap:
/// core's serde derive would render it `model3_d_generation`.
#[test]
fn stage_wire_names_match_spec() {
    assert_eq!(
        machine::wire_stage(Stage::ImageGeneration),
        "image_generation"
    );
    assert_eq!(
        machine::wire_stage(Stage::Model3DGeneration),
        "model_3d_generation"
    );
    assert_eq!(machine::wire_stage(Stage::FbxConversion), "fbx_conversion");
    assert_eq!(machine::wire_stage(Stage::Download), "download");
}

/// Progress → event mapping produces the documented state strings and fields.
#[test]
fn progress_maps_to_wire_events() {
    let ev = machine::progress_event(&Progress::Queued {
        stage: Stage::ImageGeneration,
        position: 3,
    })
    .unwrap();
    let v = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["event"], "progress");
    assert_eq!(v["stage"], "image_generation");
    assert_eq!(v["state"], "queued");
    assert_eq!(v["position"], 3);

    let ev = machine::progress_event(&Progress::Retrying {
        stage: Stage::Model3DGeneration,
        attempt: 2,
        max_attempts: 5,
        delay_secs: 10,
        reason: "rate limited".to_string(),
    })
    .unwrap();
    let v = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["state"], "retrying");
    assert_eq!(v["attempt"], 2);
    assert_eq!(v["max_attempts"], 5);
    assert_eq!(v["delay_secs"], 10);
    assert_eq!(v["reason"], "rate limited");

    // Downloading with a known total.
    let ev = machine::progress_event(&Progress::Downloading {
        stage: Stage::Download,
        bytes_downloaded: 1048576,
        total_bytes: Some(36076232),
    })
    .unwrap();
    let v = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["bytes_downloaded"], 1048576);
    assert_eq!(v["total_bytes"], 36076232);

    // total_bytes may be absent (unknown content length).
    let ev = machine::progress_event(&Progress::Downloading {
        stage: Stage::Download,
        bytes_downloaded: 512,
        total_bytes: None,
    })
    .unwrap();
    let v = serde_json::to_value(&ev).unwrap();
    assert!(
        v.get("total_bytes").is_none(),
        "total_bytes must be omitted when unknown"
    );
}

/// AwaitingApproval has no wire representation (approval is disabled under
/// --json), so it maps to None rather than an event.
#[test]
fn awaiting_approval_has_no_wire_event() {
    let progress = Progress::AwaitingApproval {
        stage: Stage::ImageGeneration,
        approval_data: asset_tap_core::types::ApprovalData {
            image_path: std::path::PathBuf::from("/x/image.png"),
            image_url: "http://x/image.png".to_string(),
            prompt: "p".to_string(),
            model: "m".to_string(),
        },
    };
    assert!(machine::progress_event(&progress).is_none());
}

/// A core `ApiError` classifies to the right kind and carries its context.
#[test]
fn api_error_classifies_with_context() {
    let api = ApiError::from_response(ApiProvider::new("fal.ai"), 401, "bad key", None);
    let err = anyhow::Error::new(CoreError::from(api));
    let wire = machine::classify_error(&err);
    assert_eq!(wire.kind, machine::KIND_UNAUTHORIZED);
    assert_eq!(wire.provider.as_deref(), Some("fal.ai"));
    assert_eq!(wire.retryable, Some(false));
    assert!(wire.action.is_some());
    // And it maps to exit code 3.
    assert_eq!(machine::exit_code_for_kind(wire.kind), 3);
}

/// Exit-code mapping matches the spec §2 table.
#[test]
fn exit_codes_match_spec_table() {
    // 3: API key missing or rejected
    assert_eq!(
        machine::exit_code_for_kind(machine::KIND_MISSING_API_KEY),
        3
    );
    assert_eq!(machine::exit_code_for_kind(machine::KIND_UNAUTHORIZED), 3);
    // 4: provider/API error
    for kind in [
        machine::KIND_PAYMENT_REQUIRED,
        machine::KIND_FORBIDDEN,
        machine::KIND_NOT_FOUND,
        machine::KIND_VALIDATION_ERROR,
        machine::KIND_RATE_LIMITED,
        machine::KIND_SERVER_ERROR,
    ] {
        assert_eq!(machine::exit_code_for_kind(kind), 4, "kind {kind}");
    }
    // 6: network/timeout
    assert_eq!(machine::exit_code_for_kind(machine::KIND_NETWORK_ERROR), 6);
    assert_eq!(machine::exit_code_for_kind(machine::KIND_TIMEOUT), 6);
    // 7: local environment/filesystem
    assert_eq!(
        machine::exit_code_for_kind(machine::KIND_BLENDER_NOT_FOUND),
        7
    );
    assert_eq!(machine::exit_code_for_kind(machine::KIND_IO_ERROR), 7);
    // 1: internal/unexpected
    assert_eq!(machine::exit_code_for_kind(machine::KIND_MODEL_ERROR), 1);
    assert_eq!(machine::exit_code_for_kind(machine::KIND_UNKNOWN), 1);
    assert_eq!(machine::exit_code_for_kind("some_future_kind"), 1);
}

/// Every declared ApiErrorKind maps to a wire kind that lands in a real exit
/// bucket — no kind falls through to a surprising code.
#[test]
fn all_api_error_kinds_have_wire_mapping() {
    for kind in [
        ApiErrorKind::Unauthorized,
        ApiErrorKind::PaymentRequired,
        ApiErrorKind::Forbidden,
        ApiErrorKind::NotFound,
        ApiErrorKind::ValidationError,
        ApiErrorKind::RateLimited,
        ApiErrorKind::ServerError,
        ApiErrorKind::Timeout,
        ApiErrorKind::ModelError,
        ApiErrorKind::NetworkError,
        ApiErrorKind::Unknown,
    ] {
        let api = ApiError {
            provider: ApiProvider::new("fal.ai"),
            kind,
            status_code: None,
            raw_message: String::new(),
            user_message: "m".to_string(),
            action: None,
            retryable: false,
            retry_after_secs: None,
            endpoint: None,
            method: None,
        };
        let err = anyhow::Error::new(CoreError::from(api));
        let wire = machine::classify_error(&err);
        // Must be one of the documented kinds (not the fallback message form).
        assert!(!wire.kind.is_empty());
        let code = machine::exit_code_for_kind(wire.kind);
        assert!((1..=7).contains(&code), "kind {kind:?} → code {code}");
    }
}

/// A KindedError (built at a CLI validation site) is honored by the classifier.
#[test]
fn kinded_error_is_classified() {
    let err = anyhow::Error::new(machine::KindedError {
        kind: machine::KIND_IO_ERROR,
        message: "Output directory cannot be empty".to_string(),
    });
    let wire = machine::classify_error(&err);
    assert_eq!(wire.kind, machine::KIND_IO_ERROR);
    assert_eq!(wire.message, "Output directory cannot be empty");
    assert_eq!(machine::exit_code_for_kind(wire.kind), 7);
}

/// Cancellation is detected via the typed core error anywhere in the chain —
/// never via message text.
#[test]
fn cancellation_is_detected() {
    let err = anyhow::Error::new(CoreError::Cancelled);
    assert!(machine::is_cancellation(&err));

    let wrapped = err.context("pipeline failed");
    assert!(machine::is_cancellation(&wrapped));

    // Provider-side cancels (ApiErrorKind::Cancelled) are cancellations too.
    let api = ApiError::from_model_error(ApiProvider::new("fal.ai"), "task was canceled");
    let provider_cancel = anyhow::Error::new(CoreError::from(api));
    assert!(machine::is_cancellation(&provider_cancel));

    // Message text no longer drives classification.
    let other = anyhow::Error::new(CoreError::Pipeline(
        "provider said: cancelled by user upstream".to_string(),
    ));
    assert!(!machine::is_cancellation(&other));
}

/// The result-error event omits absent optional fields and carries the ones
/// that are present.
#[test]
fn result_error_event_shape() {
    let wire = machine::WireError {
        kind: machine::KIND_RATE_LIMITED,
        message: "fal.ai rate limit exceeded.".to_string(),
        provider: Some("fal.ai".to_string()),
        action: Some("Request will be retried automatically.".to_string()),
        retryable: Some(true),
        retry_after_secs: Some(60),
    };
    let ev = Event::result_error(wire, Some(Stage::Model3DGeneration));
    let v = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["event"], "result");
    assert_eq!(v["status"], "error");
    assert_eq!(v["kind"], "rate_limited");
    assert_eq!(v["stage"], "model_3d_generation");
    assert_eq!(v["retry_after_secs"], 60);
    assert!(v.get("bundle_dir").is_none());
    assert!(v.get("duration_ms").is_none());
}

/// Building the catalog from the live registry succeeds and every parameter
/// definition in the bundled provider YAMLs round-trips into the wire form
/// (spec §6 acceptance: "round-trips every parameter definition"). This walks
/// the real embedded providers, not a fixture, so it fails if a new YAML
/// parameter shape can't be represented.
#[test]
fn live_catalog_round_trips_every_parameter() {
    use asset_tap_core::providers::{ProviderCapability, ProviderRegistry};

    let registry = ProviderRegistry::new();
    let catalog = machine::build_catalog(&registry, true);

    // Serializes cleanly.
    let doc = serde_json::to_string_pretty(&catalog).expect("catalog serialization failed");
    let value: Value = serde_json::from_str(&doc).unwrap();
    assert_eq!(
        value["interface"].as_str(),
        Some(machine::INTERFACE_VERSION)
    );

    // Count parameters straight from the registry and from the catalog; they
    // must match — nothing dropped in translation.
    let mut registry_param_count = 0usize;
    for provider in registry.list_all() {
        for cap in [
            ProviderCapability::TextToImage,
            ProviderCapability::ImageTo3D,
        ] {
            for model in provider.list_models(cap) {
                registry_param_count += model.parameters.len();
            }
        }
    }
    let catalog_param_count: usize = catalog
        .providers
        .iter()
        .flat_map(|p| &p.models)
        .map(|m| m.parameters.len())
        .sum();
    assert_eq!(
        registry_param_count, catalog_param_count,
        "catalog dropped parameter definitions during translation"
    );

    // Every model declares a valid modality and every parameter a valid type.
    for provider in &catalog.providers {
        for model in &provider.models {
            assert!(
                matches!(model.modality, "text_to_image" | "image_to_3d"),
                "unexpected modality {}",
                model.modality
            );
            for param in &model.parameters {
                assert!(
                    matches!(
                        param.param_type,
                        "float" | "integer" | "boolean" | "string" | "select"
                    ),
                    "unexpected param type {}",
                    param.param_type
                );
            }
        }
    }
}

/// A parameter definition serializes with all declared fields and omits unset
/// optionals (spec §3).
#[test]
fn parameter_serialization_omits_unset_optionals() {
    let def = ParameterDef {
        name: "topology".to_string(),
        label: "Topology".to_string(),
        description: None,
        param_type: ParameterType::Select,
        default: serde_json::json!("triangle"),
        min: None,
        max: None,
        step: None,
        options: Some(vec![
            serde_json::json!("triangle"),
            serde_json::json!("quad"),
        ]),
        widget: None,
    };
    let v = serde_json::to_value(machine::parameter_wire(&def)).unwrap();
    assert_eq!(v["name"], "topology");
    assert_eq!(v["type"], "select");
    assert_eq!(v["default"], "triangle");
    assert_eq!(v["options"], serde_json::json!(["triangle", "quad"]));
    assert!(v.get("description").is_none());
    assert!(v.get("min").is_none());
    assert!(v.get("widget").is_none());

    // widget is emitted when set.
    let def = ParameterDef {
        widget: Some(ParameterWidget::Input),
        ..def
    };
    let v = serde_json::to_value(machine::parameter_wire(&def)).unwrap();
    assert_eq!(v["widget"], "input");
}
