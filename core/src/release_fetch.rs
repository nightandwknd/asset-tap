//! Shared hashed download for GitHub Release artifacts (demo bundle, clip packs).
//!
//! Fail closed: a manifest without `sha256` is an error, not a skip.

use crate::bundle::{sha256_hex, verify_sha256};
use tracing::info;

const DEFAULT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Bytes of a release zip plus the manifest that authenticated them.
#[derive(Debug)]
pub(crate) struct HashedZip {
    pub bytes: Vec<u8>,
    pub manifest: serde_json::Value,
}

/// Fetch `manifest_url`, then `archive_url`, and verify SHA-256.
///
/// `on_progress` is 0.0–1.0 when `Content-Length` is known, or -1.0 while
/// the size is unknown.
pub(crate) async fn download_hashed_zip(
    manifest_url: &str,
    archive_url: &str,
    on_progress: impl Fn(f32) + Send + 'static,
) -> anyhow::Result<HashedZip> {
    let manifest = fetch_release_manifest(manifest_url).await?;
    let expected_hash = manifest_sha256(&manifest)?;
    let bytes = download_verified_bytes(archive_url, expected_hash, on_progress).await?;
    Ok(HashedZip { bytes, manifest })
}

pub(crate) async fn fetch_release_manifest(
    manifest_url: &str,
) -> anyhow::Result<serde_json::Value> {
    let client = reqwest::Client::builder()
        .timeout(DEFAULT_TIMEOUT)
        .build()?;
    info!("Checking release manifest at {manifest_url}");
    let manifest_resp = client.get(manifest_url).send().await?;
    if !manifest_resp.status().is_success() {
        anyhow::bail!(
            "Failed to fetch release manifest: HTTP {}",
            manifest_resp.status()
        );
    }
    Ok(manifest_resp.json().await?)
}

pub(crate) fn manifest_sha256(manifest: &serde_json::Value) -> anyhow::Result<&str> {
    manifest
        .get("sha256")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Release manifest is missing a sha256 hash; refusing to install unverified download"
            )
        })
}

pub(crate) async fn download_verified_bytes(
    archive_url: &str,
    expected_hash: &str,
    on_progress: impl Fn(f32) + Send + 'static,
) -> anyhow::Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .timeout(DEFAULT_TIMEOUT)
        .build()?;
    info!("Downloading {archive_url}");
    let response = client.get(archive_url).send().await?;
    if !response.status().is_success() {
        anyhow::bail!(
            "Failed to download release archive: HTTP {}",
            response.status()
        );
    }

    let total_size = response.content_length();
    let mut downloaded: u64 = 0;
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    use futures::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        downloaded += chunk.len() as u64;
        bytes.extend_from_slice(&chunk);
        match total_size {
            Some(total) => on_progress(downloaded as f32 / total as f32),
            None => on_progress(-1.0),
        }
    }
    on_progress(1.0);
    verify_sha256(&bytes, expected_hash)?;
    info!(
        "SHA-256 integrity verified ({} bytes, {})",
        bytes.len(),
        sha256_hex(&bytes)
    );
    Ok(bytes)
}

#[cfg(all(test, feature = "mock"))]
mod tests {
    use super::*;
    use crate::bundle::sha256_hex;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn hashed_zip_verifies_and_returns_bytes() {
        let zip = b"PK\x03\x04not-a-real-zip-but-hashed";
        let hash = sha256_hex(zip);
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/manifest.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sha256": hash,
                "packs_version": 1,
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/archive.zip"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(zip.as_slice()))
            .mount(&server)
            .await;

        let got = download_hashed_zip(
            &format!("{}/manifest.json", server.uri()),
            &format!("{}/archive.zip", server.uri()),
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(got.bytes, zip);
        assert_eq!(got.manifest["packs_version"], 1);
    }

    #[tokio::test]
    async fn hashed_zip_refuses_a_missing_sha() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/manifest.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "packs_version": 1
            })))
            .mount(&server)
            .await;

        let err = download_hashed_zip(
            &format!("{}/manifest.json", server.uri()),
            &format!("{}/archive.zip", server.uri()),
            |_| {},
        )
        .await
        .unwrap_err();
        assert!(
            err.to_string().contains("sha256"),
            "missing hash must fail closed: {err}"
        );
    }

    #[tokio::test]
    async fn hashed_zip_refuses_a_mismatched_hash() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/manifest.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sha256": "0".repeat(64),
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/archive.zip"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(&b"nope"[..]))
            .mount(&server)
            .await;

        let err = download_hashed_zip(
            &format!("{}/manifest.json", server.uri()),
            &format!("{}/archive.zip", server.uri()),
            |_| {},
        )
        .await
        .unwrap_err();
        assert!(
            err.to_string().contains("Integrity"),
            "bad hash must fail: {err}"
        );
    }
}
