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

/// Inline data-URI prefixes for binary payloads.
///
/// Used when a provider exposes no upload endpoint and expects the image
/// inline in the request body (e.g., Meshy's image-to-3D API).
pub mod data_uri {
    /// PNG data-URI prefix. Append base64-encoded bytes to form a full URI.
    pub const IMAGE_PNG_BASE64: &str = "data:image/png;base64,";
}

/// Upper bound on image size when falling back to data-URI encoding.
///
/// Data URIs inflate request bodies by ~33% and some providers cap request
/// size. This cap prevents surprise failures on pipelines that generate
/// unusually large intermediate images.
pub const MAX_DATA_URI_IMAGE_BYTES: usize = 10 * 1024 * 1024;

/// Environment variable names for mock mode
pub mod env {
    /// Enable mock API mode
    pub const MOCK_API: &str = "MOCK_API";

    /// Enable mock delay simulation
    pub const MOCK_DELAY: &str = "MOCK_DELAY";

    /// Enable mock failure simulation
    pub const MOCK_FAIL: &str = "MOCK_FAIL";
}
