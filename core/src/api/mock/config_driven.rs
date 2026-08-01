//! Mock handlers synthesized from a provider's own YAML.
//!
//! The handlers in [`super::generic_handlers`] match any path, but the bodies
//! they return are fal.ai's queue/status/result shapes. Providers with a
//! different contract get a 404 — Meshy returns a bare task id, polls a URL
//! built from a template, and puts results at `image_urls[0]` with no
//! `response` envelope.
//!
//! This module reads the same `PollingConfig` the HTTP client reads and builds
//! the response the client is about to ask for, honouring
//! `status_url_template`, `status_check_field`, `response_url_field`,
//! `response_envelope_field`, and `result_field`. Adding a provider YAML is
//! therefore enough to make it mock-runnable; no mock code is involved.
//!
//! **Scope.** Because the mock is derived from the same YAML that drives the
//! client, it cannot catch a YAML that misdescribes the real API — a wrong
//! `result_field` is wrong in both halves and still passes. It does exercise
//! config parsing, model registration, request bodies and parameter injection,
//! the polling loop, upload/data-URI selection, artifact download, and bundle
//! writing (see [docs/architecture/MOCK_MODE.md]). Verify response-field
//! extraction against the real API once per provider.
//!
//! [docs/architecture/MOCK_MODE.md]: ../../../../docs/architecture/MOCK_MODE.md

use super::fixtures::MockFixtures;
use super::server::{MockServerConfig, SimulatedFailure};
use crate::providers::config::{ModelConfig, PollingConfig, ProviderConfig};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

/// Highest wiremock priority (1 beats the default 5), so a config-derived
/// handler takes precedence over the catch-all fallbacks in `generic_handlers`,
/// which match every POST regardless of path.
const PRIORITY: u8 = 1;

/// Status value returned while a job is still running. Any value other than
/// the provider's `success_value`/`failure_value` works — the client only
/// compares against those two.
const PENDING_STATUS: &str = "IN_PROGRESS";

/// Which sample artifact a stage should hand back.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Artifact {
    Image,
    Model3d,
}

impl Artifact {
    /// URL of the sample file for this artifact, served by
    /// `generic_handlers::setup_file_serving`.
    fn url(self, base_url: &str) -> String {
        match self {
            Artifact::Image => MockFixtures::sample_image_url(base_url),
            Artifact::Model3d => MockFixtures::sample_model_url(base_url),
        }
    }

    fn slug(self) -> &'static str {
        match self {
            Artifact::Image => "image",
            Artifact::Model3d => "model3d",
        }
    }
}

/// Mount handlers for every polling model this provider declares.
pub async fn mount_provider(
    server: &MockServer,
    config: &ProviderConfig,
    base_url: &str,
    mock_config: &MockServerConfig,
) {
    // Several models often share one endpoint (Meshy's two text-to-image models
    // differ only by a body field) and one poll-URL template. Each distinct URL
    // is claimed once, so the poll counter isn't split across handlers.
    let mut mounted_submit: HashSet<(String, Artifact)> = HashSet::new();
    let mut mounted_poll: HashSet<(String, Artifact)> = HashSet::new();

    let models = config
        .text_to_image
        .iter()
        .map(|m| (m, Artifact::Image))
        .chain(config.image_to_3d.iter().map(|m| (m, Artifact::Model3d)));

    for (model, artifact) in models {
        let Some(polling) = model.response.polling.as_ref() else {
            // Non-polling response types fall through to the generic handlers.
            continue;
        };

        mount_model(
            server,
            model,
            polling,
            artifact,
            &config.provider.id,
            base_url,
            mock_config,
            &mut mounted_submit,
            &mut mounted_poll,
        )
        .await;
    }
}

#[allow(clippy::too_many_arguments)]
async fn mount_model(
    server: &MockServer,
    model: &ModelConfig,
    polling: &PollingConfig,
    artifact: Artifact,
    provider_id: &str,
    base_url: &str,
    mock_config: &MockServerConfig,
    mounted_submit: &mut HashSet<(String, Artifact)>,
    mounted_poll: &mut HashSet<(String, Artifact)>,
) {
    // An absolute endpoint (fal's upload/discovery URLs) isn't routed through
    // the mock's base_url rewrite, so there's nothing local to mount.
    if model.endpoint.starts_with("http://") || model.endpoint.starts_with("https://") {
        return;
    }

    let poll_count = Arc::new(AtomicU32::new(0));

    // Where the client will poll. With `status_url_template` the provider
    // dictates the URL and we mount to match it; otherwise the client reads the
    // URL out of `status_field`, so we pick one in our own namespace.
    let poll_pattern = match polling.status_url_template.as_deref() {
        Some(template) => template_to_regex(template),
        None => format!("^/__mock/{provider_id}/{}/poll/[^/]+$", artifact.slug()),
    };

    let submit_key = (model.endpoint.clone(), artifact);
    if mounted_submit.insert(submit_key) {
        mount_submit(
            server,
            model,
            polling,
            artifact,
            provider_id,
            base_url,
            mock_config,
            Arc::clone(&poll_count),
        )
        .await;
    }

    let poll_key = (poll_pattern.clone(), artifact);
    if mounted_poll.insert(poll_key) {
        mount_poll(
            server,
            polling,
            artifact,
            provider_id,
            base_url,
            mock_config,
            &poll_pattern,
            poll_count,
        )
        .await;

        // Only queue-style providers fetch the payload from a second URL.
        if polling.response_url_field.is_some() {
            mount_result(server, polling, artifact, provider_id, base_url).await;
        }
    }
}

/// POST {endpoint} — accept the job and tell the client where to poll.
#[allow(clippy::too_many_arguments)]
async fn mount_submit(
    server: &MockServer,
    model: &ModelConfig,
    polling: &PollingConfig,
    artifact: Artifact,
    provider_id: &str,
    base_url: &str,
    mock_config: &MockServerConfig,
    poll_count: Arc<AtomicU32>,
) {
    let http_method = model.method.as_str().to_string();

    if let Some(SimulatedFailure::Submit {
        status_code,
        ref message,
    }) = mock_config.simulate_failure
    {
        Mock::given(method(http_method.as_str()))
            .and(path(model.endpoint.clone()))
            .respond_with(
                ResponseTemplate::new(status_code).set_body_json(json!({ "detail": message })),
            )
            .with_priority(PRIORITY)
            .mount(server)
            .await;
        return;
    }

    let polling = polling.clone();
    let provider_id = provider_id.to_string();
    let base_url = base_url.to_string();
    let delay = mock_config.queue_delay;

    Mock::given(method(http_method.as_str()))
        .and(path(model.endpoint.clone()))
        .respond_with(move |_req: &Request| {
            // Each stage gets a fresh set of poll cycles.
            poll_count.store(0, Ordering::SeqCst);

            let task_id = MockFixtures::request_id();
            let poll_url = format!(
                "{base_url}/__mock/{provider_id}/{}/poll/{task_id}",
                artifact.slug()
            );

            let mut response = ResponseTemplate::new(200)
                .set_body_json(submit_body(&polling, &task_id, &poll_url));
            if !delay.is_zero() {
                response = response.set_delay(delay);
            }
            response
        })
        .with_priority(PRIORITY)
        .mount(server)
        .await;
}

/// GET {poll url} — report progress, then completion.
#[allow(clippy::too_many_arguments)]
async fn mount_poll(
    server: &MockServer,
    polling: &PollingConfig,
    artifact: Artifact,
    provider_id: &str,
    base_url: &str,
    mock_config: &MockServerConfig,
    poll_pattern: &str,
    poll_count: Arc<AtomicU32>,
) {
    let polling = polling.clone();
    let provider_id = provider_id.to_string();
    let base_url = base_url.to_string();
    let poll_cycles = mock_config.poll_cycles;
    let failure = mock_config.simulate_failure.clone();

    Mock::given(method("GET"))
        .and(path_regex(poll_pattern.to_string()))
        .respond_with(move |req: &Request| {
            let count = poll_count.fetch_add(1, Ordering::SeqCst);

            if let Some(SimulatedFailure::Processing {
                after_polls,
                ref message,
            }) = failure
                && count >= after_polls
            {
                return ResponseTemplate::new(200).set_body_json(failed_body(&polling, message));
            }

            if count < poll_cycles.saturating_sub(1) {
                return ResponseTemplate::new(200).set_body_json(pending_body(&polling, count));
            }

            // The task id is the last path segment of whatever the client
            // polled, so the result URL stays tied to this specific job.
            let task_id = req
                .url
                .path_segments()
                .and_then(|mut s| s.next_back())
                .unwrap_or("mock-task")
                .to_string();
            let result_url = format!(
                "{base_url}/__mock/{provider_id}/{}/result/{task_id}",
                artifact.slug()
            );

            ResponseTemplate::new(200).set_body_json(completed_body(
                &polling,
                &artifact.url(&base_url),
                &result_url,
            ))
        })
        .with_priority(PRIORITY)
        .mount(server)
        .await;
}

/// GET {result url} — the payload, for providers that split status from result.
async fn mount_result(
    server: &MockServer,
    polling: &PollingConfig,
    artifact: Artifact,
    provider_id: &str,
    base_url: &str,
) {
    let polling = polling.clone();
    let artifact_url = artifact.url(base_url);
    let pattern = format!("^/__mock/{provider_id}/{}/result/[^/]+$", artifact.slug());

    Mock::given(method("GET"))
        .and(path_regex(pattern))
        .respond_with(move |_req: &Request| {
            ResponseTemplate::new(200).set_body_json(result_body(&polling, &artifact_url))
        })
        .with_priority(PRIORITY)
        .mount(server)
        .await;
}

// =============================================================================
// Response bodies, built from the declared contract
// =============================================================================

/// Submission response: carry whatever the client needs to find the poll URL.
fn submit_body(polling: &PollingConfig, task_id: &str, poll_url: &str) -> Value {
    let mut body = json!({});
    match polling.status_url_template.as_deref() {
        // The client interpolates these fields into the template, so populate
        // exactly the ones it will look up.
        Some(template) => {
            for field in template_fields(template) {
                set_json_path(&mut body, &field, json!(task_id));
            }
        }
        None => set_json_path(&mut body, &polling.status_field, json!(poll_url)),
    }
    body
}

/// Status response for a job that hasn't finished.
fn pending_body(polling: &PollingConfig, poll_index: u32) -> Value {
    // Reuse the tqdm-style log lines so the GUI progress panel still exercises
    // block-glyph rendering offline, then stamp the provider's own status field
    // over the fal-shaped default.
    let mut body = MockFixtures::generic_status_processing_with_tqdm_logs(poll_index);
    set_json_path(
        &mut body,
        &polling.status_check_field,
        json!(PENDING_STATUS),
    );
    body
}

/// Status response for a finished job.
fn completed_body(polling: &PollingConfig, artifact_url: &str, result_url: &str) -> Value {
    let mut body = json!({});
    set_json_path(
        &mut body,
        &polling.status_check_field,
        json!(polling.success_value),
    );
    match polling.response_url_field.as_deref() {
        // Queue-style (fal): the status response only points at the payload.
        Some(field) => set_json_path(&mut body, field, json!(result_url)),
        // Task-style (Meshy): the status response *is* the payload.
        None => set_json_path(&mut body, &polling.result_field, json!(artifact_url)),
    }
    body
}

/// Payload response fetched from `response_url_field`.
fn result_body(polling: &PollingConfig, artifact_url: &str) -> Value {
    let mut payload = json!({});
    set_json_path(&mut payload, &polling.result_field, json!(artifact_url));

    let mut body = json!({});
    set_json_path(
        &mut body,
        &polling.status_check_field,
        json!(polling.success_value),
    );
    match polling.response_envelope_field.as_deref() {
        Some(envelope) => set_json_path(&mut body, envelope, payload),
        None => merge_objects(&mut body, payload),
    }
    body
}

/// Status response for a failed job.
fn failed_body(polling: &PollingConfig, message: &str) -> Value {
    let mut body = json!({ "error": message, "detail": message });
    let failed = polling.failure_value.as_deref().unwrap_or("FAILED");
    set_json_path(&mut body, &polling.status_check_field, json!(failed));
    body
}

// =============================================================================
// Path helpers
// =============================================================================

/// Field paths referenced by `${...}` tokens in a URL template.
fn template_fields(template: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find("${") {
        let after = &rest[start + 2..];
        match after.find('}') {
            Some(end) => {
                fields.push(after[..end].to_string());
                rest = &after[end + 1..];
            }
            None => break,
        }
    }
    fields
}

/// Turn a poll-URL template into an anchored regex, with each `${field}`
/// standing in for one path segment.
fn template_to_regex(template: &str) -> String {
    let mut pattern = String::from("^");
    let mut rest = template;
    while let Some(start) = rest.find("${") {
        pattern.push_str(&regex_escape(&rest[..start]));
        let after = &rest[start + 2..];
        match after.find('}') {
            Some(end) => {
                pattern.push_str("[^/]+");
                rest = &after[end + 1..];
            }
            None => {
                rest = after;
                break;
            }
        }
    }
    pattern.push_str(&regex_escape(rest));
    pattern.push('$');
    pattern
}

fn regex_escape(literal: &str) -> String {
    let mut out = String::with_capacity(literal.len());
    for ch in literal.chars() {
        if "\\.+*?()|[]{}^$".contains(ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// One step of a field path.
enum Segment {
    Key(String),
    Index(usize),
}

/// Parse the same path grammar `extract_json_field` reads: dotted keys with
/// optional array indices (`images[0].url`, `model_urls.glb`, `result`).
fn parse_path(path: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    for part in path.split('.') {
        let (name, indices) = match part.find('[') {
            Some(pos) => (&part[..pos], &part[pos..]),
            None => (part, ""),
        };
        if !name.is_empty() {
            segments.push(Segment::Key(name.to_string()));
        }
        for chunk in indices.split('[').filter(|c| !c.is_empty()) {
            if let Some(index) = chunk.strip_suffix(']').and_then(|n| n.parse().ok()) {
                segments.push(Segment::Index(index));
            }
        }
    }
    segments
}

/// Write `value` at `path`, creating intermediate objects and arrays.
///
/// The inverse of the client's field extraction: given the same path the client
/// will read, produce a body it can read it from.
fn set_json_path(root: &mut Value, path: &str, value: Value) {
    let segments = parse_path(path);
    if segments.is_empty() {
        *root = value;
        return;
    }

    let mut current = root;
    for (i, segment) in segments.iter().enumerate() {
        let last = i == segments.len() - 1;
        match segment {
            Segment::Key(key) => {
                if !current.is_object() {
                    *current = json!({});
                }
                let map = current.as_object_mut().expect("just set to object");
                if last {
                    map.insert(key.clone(), value);
                    return;
                }
                current = map.entry(key.clone()).or_insert(Value::Null);
            }
            Segment::Index(index) => {
                if !current.is_array() {
                    *current = json!([]);
                }
                let array = current.as_array_mut().expect("just set to array");
                while array.len() <= *index {
                    array.push(Value::Null);
                }
                if last {
                    array[*index] = value;
                    return;
                }
                current = &mut array[*index];
            }
        }
    }
}

/// Copy `from`'s top-level fields into `into` (both must be objects).
fn merge_objects(into: &mut Value, from: Value) {
    if let (Some(target), Value::Object(source)) = (into.as_object_mut(), from) {
        for (key, value) in source {
            target.insert(key, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn polling(yaml: &str) -> PollingConfig {
        serde_yaml_ng::from_str(yaml).expect("valid polling config")
    }

    #[test]
    fn sets_nested_and_indexed_paths() {
        let mut v = json!({});
        set_json_path(&mut v, "images[0].url", json!("u"));
        assert_eq!(v, json!({ "images": [{ "url": "u" }] }));

        let mut v = json!({});
        set_json_path(&mut v, "model_urls.glb", json!("g"));
        assert_eq!(v, json!({ "model_urls": { "glb": "g" } }));

        let mut v = json!({});
        set_json_path(&mut v, "image_urls[0]", json!("i"));
        assert_eq!(v, json!({ "image_urls": ["i"] }));
    }

    #[test]
    fn template_becomes_segment_regex() {
        assert_eq!(
            template_to_regex("/openapi/v1/text-to-image/${result}"),
            "^/openapi/v1/text-to-image/[^/]+$"
        );
        // Regex metacharacters in the literal parts are escaped; '-' is only
        // special inside a character class, so it stays as-is above.
        assert_eq!(template_to_regex("/v1.0/x/${id}"), r"^/v1\.0/x/[^/]+$");
        assert_eq!(template_fields("/x/${a}/${b.c}"), vec!["a", "b.c"]);
    }

    /// Meshy: bare task id, poll URL from a template, payload in the status
    /// response itself.
    #[test]
    fn builds_task_style_bodies() {
        let cfg = polling(
            "status_field: 'result'\n\
             status_url_template: '/openapi/v1/text-to-image/${result}'\n\
             status_check_field: 'status'\n\
             success_value: 'SUCCEEDED'\n\
             result_field: 'image_urls[0]'\n",
        );

        let submit = submit_body(&cfg, "task-1", "unused");
        assert_eq!(submit, json!({ "result": "task-1" }));

        let done = completed_body(&cfg, "http://x/img.png", "http://x/result");
        assert_eq!(done["status"], "SUCCEEDED");
        assert_eq!(done["image_urls"][0], "http://x/img.png");
    }

    /// fal: status URL handed back on submit, payload behind response_url and
    /// wrapped in an envelope.
    #[test]
    fn builds_queue_style_bodies() {
        let cfg = polling(
            "status_field: 'status_url'\n\
             status_check_field: 'status'\n\
             success_value: 'COMPLETED'\n\
             response_url_field: 'response_url'\n\
             response_envelope_field: 'response'\n\
             result_field: 'images[0].url'\n",
        );

        let submit = submit_body(&cfg, "task-1", "http://x/poll/task-1");
        assert_eq!(submit, json!({ "status_url": "http://x/poll/task-1" }));

        let done = completed_body(&cfg, "http://x/img.png", "http://x/result/task-1");
        assert_eq!(done["status"], "COMPLETED");
        assert_eq!(done["response_url"], "http://x/result/task-1");

        let result = result_body(&cfg, "http://x/img.png");
        assert_eq!(result["response"]["images"][0]["url"], "http://x/img.png");
    }

    #[test]
    fn pending_and_failed_use_the_declared_status_field() {
        let cfg = polling(
            "status_field: 'result'\n\
             status_check_field: 'state'\n\
             success_value: 'SUCCEEDED'\n\
             failure_value: 'FAILED'\n\
             result_field: 'out'\n",
        );
        assert_eq!(pending_body(&cfg, 0)["state"], PENDING_STATUS);
        assert_eq!(failed_body(&cfg, "boom")["state"], "FAILED");
    }
}
