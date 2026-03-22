//! Mock server for API testing and development mode.
//!
//! This module provides a configurable mock server that simulates
//! provider API behavior for testing and local development.
//!
//! # Usage
//!
//! ## In Tests
//!
//! ```no_run
//! use asset_tap_core::api::mock::{MockApiServer, MockServerConfig};
//!
//! #[tokio::test]
//! async fn test_with_mock() {
//!     let mock = MockApiServer::start(MockServerConfig::instant()).await;
//!     // Mock server provides provider API endpoints at mock.url()
//!     // Configure provider to use mock.url() as base_url
//! }
//! ```
//!
//! ## In Development Mode
//!
//! Set `MOCK_API=1` environment variable to enable mock mode:
//!
//! ```bash
//! MOCK_API=1 cargo run --bin asset-tap-gui
//! ```

mod fixtures;
mod generic_handlers;
mod server;

pub use fixtures::MockFixtures;
pub use server::{MockApiServer, MockServerConfig, SimulatedFailure};
