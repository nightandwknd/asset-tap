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
                    "url": format!("{}/files/generated-image.png", base_url),
                    "width": 1024,
                    "height": 1024,
                    "content_type": mime::IMAGE_PNG
                }],
                // 3D generation results (model_glb for Trellis 2 / Hunyuan3D)
                "model_glb": {
                    "url": format!("{}/files/{}", base_url, bundle_files::MODEL_GLB),
                    "content_type": mime::MODEL_GLTF_BINARY,
                    "file_name": bundle_files::MODEL_GLB,
                    "file_size": 1024000
                },
                // 3D generation results (model_mesh for Trellis v1)
                "model_mesh": {
                    "url": format!("{}/files/{}", base_url, bundle_files::MODEL_GLB),
                    "content_type": mime::MODEL_GLTF_BINARY,
                    "file_name": bundle_files::MODEL_GLB,
                    "file_size": 1024000
                },
                // Direct output URL
                "output": format!("{}/files/{}", base_url, bundle_files::MODEL_GLB)
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
/// Reads the real demo bundle assets from `bundles/asset-tap/` on disk so mock
/// mode shows the actual app icon and 3D model instead of placeholder triangles.
/// These files are never compiled into the binary — they are read at runtime
/// from the repo checkout (dev/CI only, since mock mode requires `--features mock`).
pub struct SampleFiles;

impl SampleFiles {
    /// Path to a file in the demo bundle directory.
    fn bundle_path(filename: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../bundles/asset-tap")
            .join(filename)
    }

    /// App icon PNG from the demo bundle (410KB).
    pub fn minimal_png() -> Vec<u8> {
        std::fs::read(Self::bundle_path("image.png"))
            .expect("bundles/asset-tap/image.png not found — is the repo intact?")
    }

    /// GLB from the demo bundle (~34MB, generated with TRELLIS 2).
    pub fn minimal_glb() -> Vec<u8> {
        std::fs::read(Self::bundle_path("model.glb"))
            .expect("bundles/asset-tap/model.glb not found — is the repo intact?")
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
}
