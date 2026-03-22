//! Performance benchmarks for asset-tap-core.
//!
//! Run with: `cargo bench -p asset-tap-core`
//!
//! These benchmarks measure the performance of CPU-bound operations.
//! Network operations (API calls) are not benchmarked as they depend on external services.

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};

use asset_tap_core::{
    config::generate_timestamp,
    history::{GenerationConfig, GenerationHistory, GenerationRecord, GenerationStatus},
    pipeline::PipelineConfig,
    settings::Settings,
    state::AppState,
    templates::{apply_template, list_templates},
    types::{ApiError, ApiErrorKind, ApiProvider},
};
use chrono::Utc;
use std::path::PathBuf;

/// Benchmark timestamp generation (used for output directories).
fn bench_timestamp_generation(c: &mut Criterion) {
    c.bench_function("generate_timestamp", |b| {
        b.iter(|| black_box(generate_timestamp()))
    });
}

/// Benchmark pipeline config building (builder pattern).
fn bench_pipeline_config_builder(c: &mut Criterion) {
    c.bench_function("pipeline_config_build", |b| {
        b.iter(|| {
            black_box(
                PipelineConfig::new()
                    .with_prompt("a cowboy ninja with a leather duster, bandana mask, and dual katanas on the back")
                    .with_image_model("nano-banana")
                    .with_3d_model("trellis-2"),
            )
        })
    });
}

/// Benchmark template application.
fn bench_template_application(c: &mut Criterion) {
    let templates = list_templates();

    c.bench_function("apply_template", |b| {
        b.iter(|| {
            for template in &templates {
                black_box(apply_template(template, "a fierce warrior"));
            }
        })
    });
}

/// Benchmark settings serialization (used for persistence).
fn bench_settings_serialization(c: &mut Criterion) {
    let mut settings = Settings {
        output_dir: PathBuf::from("/output"),
        ..Default::default()
    };
    settings
        .provider_api_keys
        .insert("fal.ai".to_string(), "test-key".to_string());

    let mut group = c.benchmark_group("settings");
    group.throughput(Throughput::Elements(1));

    group.bench_function("serialize", |b| {
        b.iter(|| black_box(serde_json::to_string(&settings).unwrap()))
    });

    let json = serde_json::to_string(&settings).unwrap();
    group.bench_function("deserialize", |b| {
        b.iter(|| black_box(serde_json::from_str::<Settings>(&json).unwrap()))
    });

    group.finish();
}

/// Benchmark app state serialization (used for session recovery).
fn bench_app_state_serialization(c: &mut Criterion) {
    let state = AppState {
        current_generation: Some(PathBuf::from("/output/20241229_153045")),
        preview_tab: "Model3D".to_string(),
        sidebar_collapsed: true,
        last_prompt: Some(
            "a cowboy ninja with a leather duster, bandana mask, and dual katanas on the back"
                .to_string(),
        ),
        in_progress_generation: Some("gen_12345".to_string()),
        ..Default::default()
    };

    let mut group = c.benchmark_group("app_state");
    group.throughput(Throughput::Elements(1));

    group.bench_function("serialize", |b| {
        b.iter(|| black_box(serde_json::to_string(&state).unwrap()))
    });

    let json = serde_json::to_string(&state).unwrap();
    group.bench_function("deserialize", |b| {
        b.iter(|| black_box(serde_json::from_str::<AppState>(&json).unwrap()))
    });

    group.finish();
}

/// Benchmark history operations (searching, filtering).
fn bench_history_operations(c: &mut Criterion) {
    // Create a history with many records
    let mut history = GenerationHistory::default();
    for i in 0..100 {
        let record = GenerationRecord {
            id: format!("gen_{}", i),
            started_at: Utc::now(),
            completed_at: Some(Utc::now()),
            duration_ms: Some(5000),
            status: if i % 10 == 0 {
                GenerationStatus::Failed
            } else {
                GenerationStatus::Completed
            },
            config: GenerationConfig {
                prompt: Some(format!("test prompt {} with robot character", i)),
                template: None,
                existing_image: None,
                image_model: Some("nano-banana".to_string()),
                model_3d: "trellis-2".to_string(),
                export_fbx: true,
            },
            output: None,
            error: None,
            estimated_cost: Some(0.10),
        };
        history.records.push_back(record);
    }

    let mut group = c.benchmark_group("history");

    group.bench_function("search_100_records", |b| {
        b.iter(|| black_box(history.search("robot")))
    });

    group.bench_function("filter_by_status_100_records", |b| {
        b.iter(|| black_box(history.filter_by_status(GenerationStatus::Completed)))
    });

    group.bench_function("recent_10_from_100", |b| {
        b.iter(|| {
            let recent: Vec<_> = history.recent(10).collect();
            black_box(recent)
        })
    });

    group.bench_function("stats_100_records", |b| {
        b.iter(|| black_box(history.stats()))
    });

    group.finish();
}

/// Benchmark API error creation (used in error handling paths).
fn bench_api_error_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("api_error");

    group.bench_function("from_response_401", |b| {
        b.iter(|| {
            black_box(ApiError::from_response(
                ApiProvider::new("fal.ai"),
                401,
                "Invalid API key",
                None,
            ))
        })
    });

    group.bench_function("from_response_with_detail", |b| {
        b.iter(|| {
            black_box(ApiError::from_response(
                ApiProvider::new("fal.ai"),
                422,
                "Validation error",
                Some(r#"{"detail": [{"msg": "field required", "loc": ["body", "prompt"]}]}"#),
            ))
        })
    });

    group.finish();
}

/// Benchmark error classification (determines retry behavior).
fn bench_error_classification(c: &mut Criterion) {
    let errors = vec![
        ApiError {
            provider: ApiProvider::new("fal.ai"),
            kind: ApiErrorKind::Unauthorized,
            raw_message: "Invalid API key".to_string(),
            status_code: Some(401),
            retryable: false,
            user_message: "API key is invalid".to_string(),
            action: Some("Check your API key".to_string()),
            retry_after_secs: None,
            endpoint: None,
            method: None,
        },
        ApiError {
            provider: ApiProvider::new("fal.ai"),
            kind: ApiErrorKind::RateLimited,
            raw_message: "Too many requests".to_string(),
            status_code: Some(429),
            retryable: true,
            user_message: "Rate limited".to_string(),
            action: None,
            retry_after_secs: Some(60),
            endpoint: None,
            method: None,
        },
    ];

    c.bench_function("error_is_auth_error", |b| {
        b.iter(|| {
            for err in &errors {
                black_box(err.is_auth_error());
            }
        })
    });
}

criterion_group!(
    benches,
    bench_timestamp_generation,
    bench_pipeline_config_builder,
    bench_template_application,
    bench_settings_serialization,
    bench_app_state_serialization,
    bench_history_operations,
    bench_api_error_creation,
    bench_error_classification,
);

criterion_main!(benches);
