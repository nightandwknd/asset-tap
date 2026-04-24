//! Mock API server wrapper.

use std::time::Duration;
use wiremock::MockServer;

/// Configuration for mock server behavior.
#[derive(Debug, Clone)]
pub struct MockServerConfig {
    /// Simulated delay before job starts processing.
    pub queue_delay: Duration,
    /// Simulated processing delay.
    pub processing_delay: Duration,
    /// Number of poll cycles before completion (1 = instant completion).
    pub poll_cycles: u32,
    /// Optional failure simulation for testing error handling.
    pub simulate_failure: Option<SimulatedFailure>,
}

impl Default for MockServerConfig {
    fn default() -> Self {
        Self::instant()
    }
}

impl MockServerConfig {
    /// Configuration for instant responses (testing).
    ///
    /// All operations complete immediately with no delays.
    pub fn instant() -> Self {
        Self {
            queue_delay: Duration::ZERO,
            processing_delay: Duration::ZERO,
            poll_cycles: 1,
            simulate_failure: None,
        }
    }

    /// Configuration for realistic delays (development mode).
    ///
    /// Simulates real API timing for UX testing. Each stage takes ~8-10 seconds
    /// total (2s queue delay + 5 poll cycles * ~1-2s interval), giving enough
    /// time to test cancel buttons and progress UI.
    pub fn dev_mode() -> Self {
        Self {
            queue_delay: Duration::from_secs(2),
            processing_delay: Duration::from_secs(3),
            poll_cycles: 5,
            simulate_failure: None,
        }
    }
}

/// Types of failures that can be simulated.
#[derive(Debug, Clone)]
pub enum SimulatedFailure {
    /// Fail at queue/prediction submission.
    Submit { status_code: u16, message: String },
    /// Fail during processing (after N polls).
    Processing { after_polls: u32, message: String },
}

/// Mock API server wrapping wiremock.
///
/// Provides simulated provider API endpoints for testing.
pub struct MockApiServer {
    server: MockServer,
}

impl MockApiServer {
    /// Start a new mock server with the given configuration.
    pub async fn start(config: MockServerConfig) -> Self {
        let server = MockServer::start().await;

        // Get the server URL for use in mock responses
        let base_url = server.uri();

        // Setup generic handlers that work with any provider
        super::generic_handlers::setup(&server, &config, &base_url).await;

        Self { server }
    }

    /// Get the base URL for this mock server.
    ///
    /// Use this URL when creating API clients for testing.
    pub fn url(&self) -> String {
        self.server.uri()
    }

    /// All requests the mock server has received, in order.
    ///
    /// Used by contract tests to assert request bodies match declared YAML.
    /// Returns `None` if the server was constructed with request recording
    /// disabled (default is enabled).
    pub async fn received_requests(&self) -> Option<Vec<wiremock::Request>> {
        self.server.received_requests().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instant_config() {
        let config = MockServerConfig::instant();
        assert_eq!(config.queue_delay, Duration::ZERO);
        assert_eq!(config.poll_cycles, 1);
        assert!(config.simulate_failure.is_none());
    }

    #[test]
    fn test_dev_mode_config() {
        let config = MockServerConfig::dev_mode();
        assert!(config.queue_delay > Duration::ZERO);
        assert!(config.poll_cycles > 1);
    }
}
