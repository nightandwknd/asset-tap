//! Polling and timeout constants for API operations.
//!
//! These constants control the behavior of asynchronous API polling,
//! including retry intervals and maximum attempts.

/// Default polling interval in milliseconds for text-to-image operations
pub const TEXT_TO_IMAGE_INTERVAL_MS: u64 = 1000;

/// Default polling interval in milliseconds for 3D model generation (longer due to complexity)
pub const IMAGE_TO_3D_INTERVAL_MS: u64 = 2000;

/// Default maximum polling attempts for text-to-image operations
pub const TEXT_TO_IMAGE_MAX_ATTEMPTS: u32 = 120;

/// Default maximum polling attempts for 3D model generation (longer timeout)
pub const IMAGE_TO_3D_MAX_ATTEMPTS: u32 = 180;

/// Global fallback maximum polling attempts
pub const DEFAULT_MAX_ATTEMPTS: u32 = 300;

/// Default HTTP request timeout in seconds
pub const DEFAULT_HTTP_TIMEOUT_SECS: u64 = 300;
