//! API client modules and utilities.
//!
//! This module provides utilities for API interaction:
//! - File downloads from URLs
//! - Mock mode support for testing and development
//!
//! ## Mock Mode
//!
//! For testing and development, set `MOCK_API=1` to use simulated API responses.
//! Add `MOCK_DELAY=1` for realistic timing simulation.

// Mock module is available when the 'mock' feature is enabled
#[cfg(feature = "mock")]
pub mod mock;

use crate::constants::http::env;

/// Check if mock mode is enabled via MOCK_API environment variable.
pub fn is_mock_mode() -> bool {
    std::env::var(env::MOCK_API)
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false)
}

/// Check if mock delays are enabled via MOCK_DELAY environment variable.
pub fn is_mock_delay_enabled() -> bool {
    std::env::var(env::MOCK_DELAY)
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false)
}

/// Check if mock failure simulation is enabled via MOCK_FAIL environment variable.
///
/// Set `MOCK_FAIL=1` to simulate a processing failure after a few poll cycles.
pub fn is_mock_fail_enabled() -> bool {
    std::env::var(env::MOCK_FAIL)
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false)
}

use crate::types::Result;
use std::path::Path;

/// Maximum download size (500 MB). Prevents resource exhaustion from malicious servers.
const MAX_DOWNLOAD_SIZE: u64 = 500 * 1024 * 1024;

/// Download a file from a URL to a local path and return the bytes.
///
/// Enforces a size limit to prevent resource exhaustion.
pub async fn download_file(url: &str, destination: &Path) -> Result<Vec<u8>> {
    let response = reqwest::get(url).await?.error_for_status()?;

    // Enforce size limit
    if let Some(len) = response.content_length() {
        if len > MAX_DOWNLOAD_SIZE {
            return Err(crate::types::Error::Pipeline(format!(
                "Download too large ({} bytes, max {} bytes)",
                len, MAX_DOWNLOAD_SIZE
            )));
        }
    }

    let bytes = response.bytes().await?;
    if bytes.len() as u64 > MAX_DOWNLOAD_SIZE {
        return Err(crate::types::Error::Pipeline(format!(
            "Download too large ({} bytes, max {} bytes)",
            bytes.len(),
            MAX_DOWNLOAD_SIZE
        )));
    }

    let vec = bytes.to_vec();
    std::fs::write(destination, &vec)?;
    Ok(vec)
}
