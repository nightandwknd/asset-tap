//! Generic provider-agnostic mock handlers.
//!
//! These handlers work with ANY provider configuration, not just specific providers.
//! They simulate the common patterns found in AI API providers:
//! - Queue submission → returns request_id
//! - Status polling → returns IN_QUEUE/IN_PROGRESS/COMPLETED
//! - Result data → generic structure with file URLs

use super::fixtures::{MockFixtures, SampleFiles};
use super::server::{MockServerConfig, SimulatedFailure};
use crate::constants::http::mime;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

/// Setup all generic mock handlers that work with any provider.
pub async fn setup(server: &MockServer, config: &MockServerConfig, base_url: &str) {
    // Per-server poll counter (avoids global state race between parallel tests)
    let poll_count = Arc::new(AtomicU32::new(0));

    // Order matters: more specific handlers first
    setup_discovery_endpoint(server).await;
    setup_file_serving(server).await;
    setup_upload_endpoints(server, base_url).await;
    setup_result_endpoint(server, base_url).await;
    setup_queue_submit(server, config, base_url, Arc::clone(&poll_count)).await;
    setup_status_polling(server, config, base_url, poll_count).await;
}

/// GET /v1/models - Model discovery endpoint.
///
/// Returns a mock list of available models for discovery.
async fn setup_discovery_endpoint(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path_regex(r".*/v1/models$"))
        .respond_with(|_req: &Request| {
            ResponseTemplate::new(200).set_body_json(MockFixtures::discovery_models_response())
        })
        .mount(server)
        .await;
}

/// POST /* - Generic queue submission for any endpoint.
///
/// Handles all POST requests as queue submissions, returning a request_id with URLs.
async fn setup_queue_submit(
    server: &MockServer,
    config: &MockServerConfig,
    base_url: &str,
    poll_count: Arc<AtomicU32>,
) {
    // Check for simulated failure at submission
    if let Some(SimulatedFailure::Submit {
        status_code,
        ref message,
    }) = config.simulate_failure
    {
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(status_code)
                    .set_body_json(serde_json::json!({ "detail": message })),
            )
            .mount(server)
            .await;
        return;
    }

    let delay = config.queue_delay;
    let base_url = base_url.to_string();

    Mock::given(method("POST"))
        .respond_with(move |_req: &Request| {
            // Reset poll counter so each stage gets its own full set of poll cycles
            poll_count.store(0, Ordering::SeqCst);

            let mut response = ResponseTemplate::new(200)
                .set_body_json(MockFixtures::fal_queue_response_with_urls(&base_url));
            if !delay.is_zero() {
                response = response.set_delay(delay);
            }
            response
        })
        .mount(server)
        .await;
}

/// GET /requests/*/status - Status polling endpoint.
///
/// Returns status-only responses (no result data):
/// - First N-1 polls: IN_QUEUE or IN_PROGRESS
/// - Nth poll: COMPLETED with response_url pointing to the result endpoint
async fn setup_status_polling(
    server: &MockServer,
    config: &MockServerConfig,
    base_url: &str,
    poll_count: Arc<AtomicU32>,
) {
    let poll_cycles = config.poll_cycles;
    let failure = config.simulate_failure.clone();
    let base_url = base_url.to_string();

    // Match status check URLs: /requests/{id}/status
    Mock::given(method("GET"))
        .and(path_regex(r".*/requests/.*/status$"))
        .respond_with(move |_req: &Request| {
            let count = poll_count.fetch_add(1, Ordering::SeqCst);

            // Check for processing failure
            if let Some(SimulatedFailure::Processing {
                after_polls,
                ref message,
            }) = failure
                && count >= after_polls
            {
                return ResponseTemplate::new(200)
                    .set_body_json(MockFixtures::generic_status_failed(message));
            }

            // Determine status based on poll count
            if count < poll_cycles.saturating_sub(1) {
                if count == 0 {
                    ResponseTemplate::new(200)
                        .set_body_json(MockFixtures::generic_status_queued(poll_cycles - count))
                } else {
                    // Attach tqdm-style log lines (accumulating) so the progress
                    // panel exercises block-element glyph rendering offline.
                    ResponseTemplate::new(200).set_body_json(
                        MockFixtures::generic_status_processing_with_tqdm_logs(count - 1),
                    )
                }
            } else {
                // Completed - return status with response_url
                let request_id = "mock-request-id";
                ResponseTemplate::new(200).set_body_json(MockFixtures::generic_completed_status(
                    &base_url, request_id,
                ))
            }
        })
        .mount(server)
        .await;
}

/// GET /requests/* - Result endpoint (fetched after COMPLETED status).
///
/// Returns the actual model output (images, 3D models, etc.).
async fn setup_result_endpoint(server: &MockServer, base_url: &str) {
    let base_url = base_url.to_string();

    // Match result URLs: /requests/{id} (but NOT /requests/{id}/status)
    Mock::given(method("GET"))
        .and(path_regex(r".*/requests/[^/]+$"))
        .respond_with(move |_req: &Request| {
            ResponseTemplate::new(200)
                .set_body_json(MockFixtures::generic_result_response(&base_url))
        })
        .mount(server)
        .await;
}

/// GET /files/* - Serve sample files (PNG, GLB).
async fn setup_file_serving(server: &MockServer) {
    // Serve PNG files
    Mock::given(method("GET"))
        .and(path_regex(r".*\.(png|jpg|jpeg)$"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(SampleFiles::minimal_png())
                .insert_header("content-type", mime::IMAGE_PNG),
        )
        .mount(server)
        .await;

    // Serve GLB files
    Mock::given(method("GET"))
        .and(path_regex(r".*\.glb$"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(SampleFiles::minimal_glb())
                .insert_header("content-type", mime::MODEL_GLTF_BINARY),
        )
        .mount(server)
        .await;
}

/// POST /storage/upload/*, PUT /* - Generic file upload endpoints.
async fn setup_upload_endpoints(server: &MockServer, base_url: &str) {
    let base_url = base_url.to_string();

    // Upload initiation (returns upload_url and file_url)
    Mock::given(method("POST"))
        .and(path_regex(r".*/upload(/initiate)?$"))
        .respond_with(move |_req: &Request| {
            ResponseTemplate::new(200)
                .set_body_json(MockFixtures::generic_upload_initiate(&base_url))
        })
        .mount(server)
        .await;

    // File upload (PUT to upload URL)
    Mock::given(method("PUT"))
        .respond_with(ResponseTemplate::new(200))
        .mount(server)
        .await;
}
