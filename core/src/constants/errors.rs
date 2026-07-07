//! Error handling constants.
//!
//! This module defines retry delays, error patterns, and other error-related constants.

/// Retry delay for rate limiting errors (HTTP 429) in seconds
pub const RATE_LIMIT_RETRY_DELAY_SECS: u64 = 30;

/// Retry delay for general server errors (HTTP 500) in seconds
pub const SERVER_ERROR_RETRY_DELAY_SECS: u64 = 5;

/// Retry delay for bad gateway errors (HTTP 502, 503) in seconds
pub const BAD_GATEWAY_RETRY_DELAY_SECS: u64 = 10;

/// Retry delay for gateway timeout errors (HTTP 504) in seconds
pub const GATEWAY_TIMEOUT_RETRY_DELAY_SECS: u64 = 15;

/// Maximum number of *consecutive* transient failures (network error, or a
/// 5xx/timeout while polling) tolerated before a polling operation gives up.
/// A paid task has already been submitted at this point, so we retry through
/// brief blips rather than abandoning (and leaking) the remote job on the
/// first hiccup — but cap it so a persistently unreachable provider still
/// terminates instead of polling forever.
pub const MAX_CONSECUTIVE_POLL_FAILURES: u32 = 5;

/// Base backoff (seconds) between consecutive transient poll failures. Grows
/// exponentially (base, 2×, 4×, …) capped at [`MAX_POLL_RETRY_BACKOFF_SECS`].
pub const POLL_RETRY_BASE_BACKOFF_SECS: u64 = 2;

/// Upper bound on the exponential poll-retry backoff, in seconds.
pub const MAX_POLL_RETRY_BACKOFF_SECS: u64 = 30;

/// Error patterns for detecting specific error types
pub mod patterns {
    /// Out of memory error patterns
    pub const OOM_PATTERNS: &[&str] = &["OOM", "out of memory", "E1001"];

    /// Timeout error patterns
    pub const TIMEOUT_PATTERNS: &[&str] = &["timeout", "E6716"];

    /// Health check failure patterns
    pub const HEALTH_CHECK_PATTERNS: &[&str] = &["E8765", "health check"];
}
