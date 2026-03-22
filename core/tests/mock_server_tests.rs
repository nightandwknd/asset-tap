//! Mock server functionality tests.
//!
//! These tests validate that the mock API server works correctly
//! for testing and development mode.

use asset_tap_core::api::mock::{MockApiServer, MockServerConfig, SimulatedFailure};
use asset_tap_core::constants::http::{env, mime};

// =============================================================================
// Mock Server Startup Tests
// =============================================================================

#[tokio::test]
async fn test_mock_server_starts_successfully() {
    let mock = MockApiServer::start(MockServerConfig::instant()).await;
    let url = mock.url();

    // Verify we got a valid URL
    assert!(url.starts_with("http://"));
    assert!(url.contains("127.0.0.1") || url.contains("localhost"));
}

#[tokio::test]
async fn test_mock_server_serves_files() {
    let mock = MockApiServer::start(MockServerConfig::instant()).await;
    let client = reqwest::Client::new();

    // Test PNG file serving
    let response = client
        .get(format!("{}/files/test.png", mock.url()))
        .send()
        .await
        .expect("Mock server should respond");

    assert_eq!(response.status(), 200);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        asset_tap_core::constants::http::mime::IMAGE_PNG,
    );

    let bytes = response.bytes().await.unwrap();
    // Check PNG signature
    assert_eq!(bytes[0], 0x89);
    assert_eq!(bytes[1], 0x50);
    assert_eq!(bytes[2], 0x4E);
    assert_eq!(bytes[3], 0x47);
}

#[tokio::test]
async fn test_mock_server_with_delay_config() {
    let mock = MockApiServer::start(MockServerConfig::dev_mode()).await;
    let url = mock.url();

    assert!(url.starts_with("http://"));
}

// =============================================================================
// Environment Variable Tests
// =============================================================================

#[test]
fn test_mock_mode_detection() {
    use asset_tap_core::api::is_mock_mode;

    // Test various values
    std::env::remove_var(env::MOCK_API);
    assert!(!is_mock_mode());

    std::env::set_var(env::MOCK_API, "1");
    assert!(is_mock_mode());

    std::env::set_var(env::MOCK_API, "true");
    assert!(is_mock_mode());

    std::env::set_var(env::MOCK_API, "0");
    assert!(!is_mock_mode());

    std::env::set_var(env::MOCK_API, "false");
    assert!(!is_mock_mode());

    // Cleanup
    std::env::remove_var(env::MOCK_API);
}

#[test]
fn test_mock_delay_detection() {
    use asset_tap_core::api::is_mock_delay_enabled;

    std::env::remove_var(env::MOCK_DELAY);
    assert!(!is_mock_delay_enabled());

    std::env::set_var(env::MOCK_DELAY, "1");
    assert!(is_mock_delay_enabled());

    std::env::set_var(env::MOCK_DELAY, "true");
    assert!(is_mock_delay_enabled());

    // Cleanup
    std::env::remove_var(env::MOCK_DELAY);
}

// =============================================================================
// Mock API Endpoint Tests
// =============================================================================

#[tokio::test]
async fn test_fal_ai_queue_submission() {
    let mock = MockApiServer::start(MockServerConfig::instant()).await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/fal-ai/nano-banana", mock.url()))
        .json(&serde_json::json!({
            "prompt": "test prompt"
        }))
        .send()
        .await
        .expect("Request should succeed");

    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await.unwrap();

    // Verify response structure
    assert!(body.get("request_id").is_some(), "Should have request_id");
}

#[tokio::test]
async fn test_generic_prediction_creation() {
    let mock = MockApiServer::start(MockServerConfig::instant()).await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/v1/predictions", mock.url()))
        .json(&serde_json::json!({
            "version": "test-version",
            "input": {
                "prompt": "test"
            }
        }))
        .send()
        .await
        .expect("Request should succeed");

    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await.unwrap();

    // Generic handler returns request_id for provider-agnostic testing
    assert!(body.get("request_id").is_some(), "Should have request_id");
}

// =============================================================================
// Polling Simulation Tests
// =============================================================================

#[tokio::test]
async fn test_polling_progression() {
    let config = MockServerConfig {
        queue_delay: std::time::Duration::ZERO,
        processing_delay: std::time::Duration::ZERO,
        poll_cycles: 3,
        simulate_failure: None,
    };

    let mock = MockApiServer::start(config).await;
    let client = reqwest::Client::new();

    // Submit job
    let response = client
        .post(format!("{}/fal-ai/test-model", mock.url()))
        .json(&serde_json::json!({"prompt": "test"}))
        .send()
        .await
        .unwrap();

    let body: serde_json::Value = response.json().await.unwrap();
    let request_id = body["request_id"].as_str().unwrap();

    // Poll multiple times
    for poll_num in 0..3 {
        let response = client
            .get(format!(
                "{}/fal-ai/test-model/requests/{}/status",
                mock.url(),
                request_id
            ))
            .send()
            .await
            .unwrap();

        let body: serde_json::Value = response.json().await.unwrap();
        let status = body["status"].as_str().unwrap();

        if poll_num < 2 {
            // First polls should show queued/processing
            assert!(
                status == "IN_QUEUE" || status == "IN_PROGRESS",
                "Poll {} should be in progress, got: {}",
                poll_num,
                status
            );
        } else {
            // Final poll should be completed
            assert_eq!(status, "COMPLETED", "Final poll should be completed");
        }
    }
}

// =============================================================================
// Failure Simulation Tests
// =============================================================================

#[tokio::test]
async fn test_simulated_submit_failure() {
    let config = MockServerConfig {
        queue_delay: std::time::Duration::ZERO,
        processing_delay: std::time::Duration::ZERO,
        poll_cycles: 1,
        simulate_failure: Some(SimulatedFailure::Submit {
            status_code: 401,
            message: "Unauthorized".to_string(),
        }),
    };

    let mock = MockApiServer::start(config).await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/fal-ai/test", mock.url()))
        .json(&serde_json::json!({"prompt": "test"}))
        .send()
        .await
        .expect("Request should complete");

    assert_eq!(response.status(), 401, "Should return 401 error");
}

#[tokio::test]
async fn test_simulated_processing_failure() {
    let config = MockServerConfig {
        queue_delay: std::time::Duration::ZERO,
        processing_delay: std::time::Duration::ZERO,
        poll_cycles: 2,
        simulate_failure: Some(SimulatedFailure::Processing {
            after_polls: 1,
            message: "GPU out of memory".to_string(),
        }),
    };

    let mock = MockApiServer::start(config).await;
    let client = reqwest::Client::new();

    // Submit should succeed
    let submit = client
        .post(format!("{}/fal-ai/test", mock.url()))
        .json(&serde_json::json!({"prompt": "test"}))
        .send()
        .await
        .expect("Submit should succeed");

    assert_eq!(submit.status(), 200);
    let body: serde_json::Value = submit.json().await.unwrap();
    let request_id = body["request_id"].as_str().unwrap();

    // First poll (count=0) should return IN_QUEUE (before after_polls threshold)
    let poll1 = client
        .get(format!(
            "{}/fal-ai/test/requests/{}/status",
            mock.url(),
            request_id
        ))
        .send()
        .await
        .unwrap();
    let poll1_body: serde_json::Value = poll1.json().await.unwrap();
    assert_eq!(poll1_body["status"], "IN_QUEUE");

    // Second poll (count=1, >= after_polls) should return FAILED
    let poll2 = client
        .get(format!(
            "{}/fal-ai/test/requests/{}/status",
            mock.url(),
            request_id
        ))
        .send()
        .await
        .unwrap();
    let poll2_body: serde_json::Value = poll2.json().await.unwrap();
    assert_eq!(poll2_body["status"], "FAILED");
    assert!(poll2_body["error"]
        .as_str()
        .unwrap()
        .contains("GPU out of memory"));
}

// =============================================================================
// File Upload Mock Tests
// =============================================================================

#[tokio::test]
async fn test_fal_upload_initiate() {
    let mock = MockApiServer::start(MockServerConfig::instant()).await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/storage/upload/initiate", mock.url()))
        .json(&serde_json::json!({
            "file_name": "test.png",
            "content_type": mime::IMAGE_PNG
        }))
        .send()
        .await
        .expect("Request should succeed");

    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await.unwrap();

    assert!(body.get("upload_url").is_some(), "Should have upload_url");
    assert!(body.get("file_url").is_some(), "Should have file_url");
}

#[tokio::test]
async fn test_file_upload() {
    let mock = MockApiServer::start(MockServerConfig::instant()).await;
    let client = reqwest::Client::new();

    // Get upload URL
    let init_response = client
        .post(format!("{}/storage/upload/initiate", mock.url()))
        .json(&serde_json::json!({
            "file_name": "test.png",
            "content_type": mime::IMAGE_PNG
        }))
        .send()
        .await
        .unwrap();

    let body: serde_json::Value = init_response.json().await.unwrap();
    let upload_url = body["upload_url"].as_str().unwrap();

    // Upload file
    let upload_response = client
        .put(upload_url)
        .body(vec![1, 2, 3, 4, 5])
        .send()
        .await
        .expect("Upload should succeed");

    assert_eq!(upload_response.status(), 200);
}

// =============================================================================
// Mock Fixtures Tests
// =============================================================================

#[test]
fn test_mock_fixtures_structure() {
    use asset_tap_core::api::mock::MockFixtures;

    // Test fal.ai queue response
    let queue_resp = MockFixtures::fal_queue_response();
    assert!(queue_resp.get("request_id").is_some());
    // Queue response only has request_id, not status
}
