//! Mock response fixtures and sample files.

use crate::constants::files::bundle as bundle_files;
use crate::constants::http::mime;
use serde_json::{Value, json};

/// Mock response fixtures for API endpoints.
pub struct MockFixtures;

impl MockFixtures {
    /// Generate a unique request ID.
    pub fn request_id() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    /// URL of the sample image served by `generic_handlers::setup_file_serving`.
    ///
    /// Response builders in both `fixtures` and `config_driven` hand this URL
    /// to the client, so it lives in one place — a rename here can't leave one
    /// of them pointing at a path nothing serves.
    pub fn sample_image_url(base_url: &str) -> String {
        format!("{base_url}/files/generated-image.png")
    }

    /// URL of the sample GLB served by `generic_handlers::setup_file_serving`.
    pub fn sample_model_url(base_url: &str) -> String {
        format!("{base_url}/files/{}", bundle_files::MODEL_GLB)
    }

    // =========================================================================
    // Generic Responses (Provider-Agnostic)
    // =========================================================================

    /// Generic status: job completed (status response only, no result data).
    ///
    /// Used by the status polling endpoint. The actual result is fetched
    /// separately from the response_url.
    pub fn generic_completed_status(base_url: &str, request_id: &str) -> Value {
        json!({
            "status": "COMPLETED",
            "request_id": request_id,
            "response_url": format!("{}/requests/{}", base_url, request_id),
            "status_url": format!("{}/requests/{}/status", base_url, request_id)
        })
    }

    /// Generic result response (fetched from response_url after COMPLETED status).
    ///
    /// Matches the real fal.ai response_url format: model output is wrapped
    /// in a "response" envelope field, alongside status and logs.
    /// result_field patterns (e.g. `images[0].url`, `model_glb.url`) apply
    /// to the inner "response" object.
    pub fn generic_result_response(base_url: &str) -> Value {
        json!({
            "status": "COMPLETED",
            "response": {
                // Image generation result
                "images": [{
                    "url": Self::sample_image_url(base_url),
                    "width": 1024,
                    "height": 1024,
                    "content_type": mime::IMAGE_PNG
                }],
                // 3D generation results (model_glb for Trellis 2 / Hunyuan3D)
                "model_glb": {
                    "url": Self::sample_model_url(base_url),
                    "content_type": mime::MODEL_GLTF_BINARY,
                    "file_name": bundle_files::MODEL_GLB,
                    "file_size": 1024000
                },
                // 3D generation results (model_mesh for Trellis v1)
                "model_mesh": {
                    "url": Self::sample_model_url(base_url),
                    "content_type": mime::MODEL_GLTF_BINARY,
                    "file_name": bundle_files::MODEL_GLB,
                    "file_size": 1024000
                },
                // Direct output URL
                "output": Self::sample_model_url(base_url)
            }
        })
    }

    /// Generic status: job is queued.
    pub fn generic_status_queued(position: u32) -> Value {
        json!({
            "status": "IN_QUEUE",
            "queue_position": position
        })
    }

    /// Generic status: job is processing.
    pub fn generic_status_processing() -> Value {
        json!({
            "status": "IN_PROGRESS"
        })
    }

    /// Generic status: job is processing, with a tqdm-style log array.
    ///
    /// Mirrors the `logs` array shape fal.ai returns on `?logs=1` polling. Each
    /// call produces a `Progress::Log` entry on the GUI/CLI side, letting us
    /// exercise block-element glyph rendering (U+2588 and friends) offline.
    /// `poll_index` is zero-based; logs accumulate across polls the same way
    /// real providers return them.
    pub fn generic_status_processing_with_tqdm_logs(poll_index: u32) -> Value {
        const TQDM_LOG_LINES: &[&str] = &[
            "Sampling texture SLat:   8%|\u{258F}         | 1/12 [00:00<00:01,  8.25it/s]",
            "Sampling texture SLat:  25%|\u{2588}\u{2588}\u{258C}       | 3/12 [00:00<00:01,  8.25it/s]",
            "Sampling texture SLat:  50%|\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}     | 6/12 [00:00<00:00,  8.23it/s]",
            "Sampling texture SLat:  75%|\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{258C}  | 9/12 [00:01<00:00,  8.25it/s]",
            "Sampling texture SLat: 100%|\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}| 12/12 [00:01<00:00,  8.19it/s]",
        ];

        let lines_to_emit = ((poll_index as usize) + 1).min(TQDM_LOG_LINES.len());
        let logs: Vec<Value> = TQDM_LOG_LINES[..lines_to_emit]
            .iter()
            .map(|msg| json!({ "message": msg, "level": "INFO", "source": "mock" }))
            .collect();

        json!({
            "status": "IN_PROGRESS",
            "logs": logs,
        })
    }

    /// Generic status: job failed.
    pub fn generic_status_failed(error: &str) -> Value {
        json!({
            "status": "FAILED",
            "error": error
        })
    }

    /// Generic upload initiation response.
    pub fn generic_upload_initiate(base_url: &str) -> Value {
        let file_id = Self::request_id();
        json!({
            "upload_url": format!("{}/mock-upload/{}", base_url, file_id),
            "file_url": format!("{}/files/uploaded-{}.png", base_url, file_id)
        })
    }

    // =========================================================================
    // Test-Only Fixtures
    // =========================================================================

    /// Queue submission response (generic, used by generic_handlers and tests).
    pub fn fal_queue_response_with_urls(base_url: &str) -> Value {
        let request_id = Self::request_id();
        json!({
            "request_id": &request_id,
            "status_url": format!("{}/requests/{}/status", base_url, request_id),
            "response_url": format!("{}/requests/{}", base_url, request_id),
            "status": "IN_QUEUE"
        })
    }

    /// Queue submission response (minimal, without URLs).
    pub fn fal_queue_response() -> Value {
        json!({
            "request_id": Self::request_id()
        })
    }

    /// Model discovery endpoint response.
    ///
    /// Returns a generic list of models for discovery, compatible with fal.ai format.
    pub fn discovery_models_response() -> Value {
        json!({
            "models": [
                {
                    "endpoint_id": "mock-text-to-image",
                    "metadata": {
                        "display_name": "Mock Text to Image",
                        "description": "Mock model for text to image generation",
                        "status": "active"
                    }
                },
                {
                    "endpoint_id": "mock-image-to-3d",
                    "metadata": {
                        "display_name": "Mock Image to 3D",
                        "description": "Mock model for image to 3D conversion",
                        "status": "active"
                    }
                }
            ]
        })
    }
}

/// Sample binary files for mock downloads.
///
/// Prefers the real demo bundle assets from `bundles/asset-tap/` on disk so
/// dev-checkout mock runs show the actual app icon and 3D model. The disk path
/// is resolved via `env!("CARGO_MANIFEST_DIR")`, which is baked in at compile
/// time — in released binaries it points at the build machine's workspace and
/// never exists on user machines. Since releases ship with `--features mock`,
/// missing disk assets fall back to small embedded placeholders (a solid-color
/// PNG and a unit-cube GLB) instead of panicking, so `--mock` works anywhere.
pub struct SampleFiles;

/// Embedded fallback image: 64×64 solid-color PNG (~136 bytes).
const PLACEHOLDER_PNG: &[u8] = include_bytes!("assets/placeholder.png");

/// Embedded fallback model: minimal valid glTF 2.0 binary, a unit cube (~772 bytes).
const PLACEHOLDER_GLB: &[u8] = include_bytes!("assets/placeholder.glb");

/// When set, skip the on-disk demo assets and serve the embedded placeholders.
/// Test-only knob: lets CI exercise the released-binary fallback path from a
/// repo checkout, where the disk assets would otherwise always win.
pub const MOCK_EMBEDDED_ENV: &str = "ASSET_TAP_MOCK_EMBEDDED";

impl SampleFiles {
    /// Path to a file in the demo bundle directory (build-machine path; see
    /// the struct docs for why this can be absent at runtime).
    fn bundle_path(filename: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../bundles/asset-tap")
            .join(filename)
    }

    /// Demo bundle asset from disk, unless absent or overridden.
    fn disk_asset(filename: &str) -> Option<Vec<u8>> {
        if std::env::var_os(MOCK_EMBEDDED_ENV).is_some() {
            return None;
        }
        std::fs::read(Self::bundle_path(filename)).ok()
    }

    /// Sample image: the demo bundle's PNG (~410KB) when the repo checkout is
    /// present, else the embedded placeholder.
    pub fn minimal_png() -> Vec<u8> {
        Self::disk_asset("image.png").unwrap_or_else(|| PLACEHOLDER_PNG.to_vec())
    }

    /// Sample model: the demo bundle's GLB (~34MB, generated with TRELLIS 2)
    /// when the repo checkout is present, else the embedded placeholder.
    pub fn minimal_glb() -> Vec<u8> {
        Self::disk_asset("model.glb").unwrap_or_else(|| PLACEHOLDER_GLB.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_id_unique() {
        let id1 = MockFixtures::request_id();
        let id2 = MockFixtures::request_id();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_fal_queue_response() {
        let response = MockFixtures::fal_queue_response();
        assert!(response.get("request_id").is_some());
    }

    #[test]
    fn test_minimal_png_valid() {
        // SampleFiles observes MOCK_EMBEDDED_ENV, so hold the env lock
        let _env = crate::test_support::env_lock();
        let png = SampleFiles::minimal_png();
        // PNG files start with these magic bytes
        assert_eq!(
            &png[0..8],
            &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
        );
        // Demo bundle image should be several hundred KB
        assert!(
            png.len() > 100_000,
            "PNG should be at least 100KB, got {}",
            png.len()
        );
    }

    #[test]
    fn test_minimal_glb_valid() {
        // SampleFiles observes MOCK_EMBEDDED_ENV, so hold the env lock
        let _env = crate::test_support::env_lock();
        let glb = SampleFiles::minimal_glb();
        // GLB files start with "glTF" magic
        assert_eq!(&glb[0..4], b"glTF");
        // Version 2
        assert_eq!(glb[4], 2);
        // Must be at least a valid GLB header (12 bytes)
        assert!(
            glb.len() >= 12,
            "GLB should be at least 12 bytes, got {}",
            glb.len()
        );
    }

    #[test]
    fn test_placeholder_png_valid() {
        assert_eq!(
            &PLACEHOLDER_PNG[0..8],
            &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
        );
        // Trailing IEND chunk: type tag sits 8 bytes from the end (before the CRC)
        assert_eq!(&PLACEHOLDER_PNG[PLACEHOLDER_PNG.len() - 8..][..4], b"IEND");
    }

    #[test]
    fn test_placeholder_glb_valid() {
        assert_eq!(&PLACEHOLDER_GLB[0..4], b"glTF");
        let version = u32::from_le_bytes(PLACEHOLDER_GLB[4..8].try_into().unwrap());
        assert_eq!(version, 2);
        // Declared total length must match the actual byte count
        let total = u32::from_le_bytes(PLACEHOLDER_GLB[8..12].try_into().unwrap());
        assert_eq!(total as usize, PLACEHOLDER_GLB.len());
        // JSON chunk must parse and declare glTF 2.0
        let json_len = u32::from_le_bytes(PLACEHOLDER_GLB[12..16].try_into().unwrap()) as usize;
        let doc: Value = serde_json::from_slice(&PLACEHOLDER_GLB[20..20 + json_len]).unwrap();
        assert_eq!(doc["asset"]["version"], "2.0");
    }

    #[test]
    fn test_embedded_env_forces_placeholders() {
        let _env = crate::test_support::env_lock();
        unsafe { std::env::set_var(MOCK_EMBEDDED_ENV, "1") };
        let png = SampleFiles::minimal_png();
        let glb = SampleFiles::minimal_glb();
        unsafe { std::env::remove_var(MOCK_EMBEDDED_ENV) };
        assert_eq!(png, PLACEHOLDER_PNG);
        assert_eq!(glb, PLACEHOLDER_GLB);
    }
}
