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
/// Uses an explicit request timeout (the bare `reqwest::get` client has none,
/// so a stalled server would hang the pipeline forever) and streams the body,
/// enforcing the size cap as bytes arrive so a server that lies about or omits
/// Content-Length cannot exhaust memory.
pub async fn download_file(url: &str, destination: &Path) -> Result<Vec<u8>> {
    use crate::constants::polling::DEFAULT_HTTP_TIMEOUT_SECS;
    use futures::StreamExt as _;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(DEFAULT_HTTP_TIMEOUT_SECS))
        .build()
        .map_err(|e| crate::types::Error::Pipeline(format!("HTTP client build failed: {e}")))?;

    let response = client.get(url).send().await?.error_for_status()?;

    let content_length = response.content_length();
    if let Some(len) = content_length
        && len > MAX_DOWNLOAD_SIZE
    {
        return Err(crate::types::Error::Pipeline(format!(
            "Download too large ({} bytes, max {} bytes)",
            len, MAX_DOWNLOAD_SIZE
        )));
    }

    let mut buf: Vec<u8> = Vec::with_capacity(
        content_length
            .map(|l| l.min(MAX_DOWNLOAD_SIZE) as usize)
            .unwrap_or(0),
    );
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if buf.len() as u64 + chunk.len() as u64 > MAX_DOWNLOAD_SIZE {
            return Err(crate::types::Error::Pipeline(format!(
                "Download too large (exceeded max {} bytes)",
                MAX_DOWNLOAD_SIZE
            )));
        }
        buf.extend_from_slice(&chunk);
    }

    tokio::fs::write(destination, &buf).await?;
    Ok(buf)
}
