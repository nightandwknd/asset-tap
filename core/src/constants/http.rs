//! HTTP-related constants.
//!
//! Constants for HTTP operations including byte size thresholds,
//! MIME types, and header names.

/// Bytes per kilobyte
pub const BYTES_PER_KB: u64 = 1024;

/// Bytes per megabyte
pub const BYTES_PER_MB: u64 = 1024 * 1024;

/// Standard MIME types used by Asset Tap
pub mod mime {
    /// PNG image MIME type
    pub const IMAGE_PNG: &str = "image/png";

    /// WebP image MIME type
    pub const IMAGE_WEBP: &str = "image/webp";

    /// GLB (glTF binary) MIME type
    pub const MODEL_GLTF_BINARY: &str = "model/gltf-binary";
}

/// Standard HTTP header names
pub mod headers {
    /// Authorization header
    pub const AUTHORIZATION: &str = "Authorization";

    /// Content-Type header
    pub const CONTENT_TYPE: &str = "Content-Type";
}

/// Environment variable names for mock mode
pub mod env {
    /// Enable mock API mode
    pub const MOCK_API: &str = "MOCK_API";

    /// Enable mock delay simulation
    pub const MOCK_DELAY: &str = "MOCK_DELAY";

    /// Enable mock failure simulation
    pub const MOCK_FAIL: &str = "MOCK_FAIL";
}
