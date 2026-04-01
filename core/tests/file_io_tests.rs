//! File I/O operation tests.
//!
//! These tests validate file download, bundle creation, and file system operations.

use asset_tap_core::{
    bundle::BundleMetadata, config::generate_timestamp, constants::files::bundle as bundle_files,
    history::GenerationConfig,
};
use std::fs;
use tempfile::TempDir;

// =============================================================================
// File Download Tests (require mock feature for wiremock)
// =============================================================================

#[cfg(feature = "mock")]
#[tokio::test]
async fn test_download_file_success() {
    use asset_tap_core::api::download_file;
    // Use a local mock server instead of network dependency
    let temp_dir = TempDir::new().unwrap();
    let dest_path = temp_dir.path().join("downloaded.bin");

    // Create a source file and serve it via a local mock server
    let mock_server = wiremock::MockServer::start().await;
    let body = vec![0x42u8; 100];
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/test.bin"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes(body.clone()))
        .mount(&mock_server)
        .await;

    let url = format!("{}/test.bin", mock_server.uri());
    let result = download_file(&url, &dest_path).await;

    assert!(result.is_ok(), "Download should succeed");
    let bytes = result.unwrap();

    assert_eq!(bytes.len(), 100, "Should download 100 bytes");
    assert!(dest_path.exists(), "File should be created");

    let file_contents = fs::read(&dest_path).unwrap();
    assert_eq!(bytes, file_contents, "Returned bytes should match file");
}

#[cfg(feature = "mock")]
#[tokio::test]
async fn test_download_file_to_nested_directory() {
    use asset_tap_core::api::download_file;

    let temp_dir = TempDir::new().unwrap();
    let nested_path = temp_dir
        .path()
        .join("a")
        .join("b")
        .join("c")
        .join("file.bin");

    fs::create_dir_all(nested_path.parent().unwrap()).unwrap();

    let mock_server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/file.bin"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes(vec![0x01; 50]))
        .mount(&mock_server)
        .await;

    let url = format!("{}/file.bin", mock_server.uri());
    let result = download_file(&url, &nested_path).await;

    assert!(result.is_ok(), "Download to nested path should succeed");
    assert!(
        nested_path.exists(),
        "File should exist in nested directory"
    );
}

#[cfg(feature = "mock")]
#[tokio::test]
async fn test_download_file_invalid_url() {
    use asset_tap_core::api::download_file;

    let temp_dir = TempDir::new().unwrap();
    let dest_path = temp_dir.path().join("failed.bin");

    let url = "https://this-domain-definitely-does-not-exist-12345.com/file";
    let result = download_file(url, &dest_path).await;

    assert!(result.is_err(), "Download should fail for invalid URL");
}

#[cfg(feature = "mock")]
#[tokio::test]
async fn test_download_file_404() {
    use asset_tap_core::api::download_file;

    let mock_server = wiremock::MockServer::start().await;
    // No routes mounted — any request returns 404

    let temp_dir = TempDir::new().unwrap();
    let dest_path = temp_dir.path().join("notfound.bin");

    let url = format!("{}/missing.bin", mock_server.uri());
    let result = download_file(&url, &dest_path).await;

    assert!(result.is_err(), "Download should fail for 404");
    assert!(!dest_path.exists(), "File should not be created on 404");
}

// =============================================================================
// Bundle Creation Tests
// =============================================================================

#[test]
fn test_bundle_metadata_save_and_load() {
    let temp_dir = TempDir::new().unwrap();
    let bundle_dir = temp_dir.path().join("test_bundle");
    fs::create_dir_all(&bundle_dir).unwrap();

    // Create metadata with generation config
    let config = GenerationConfig {
        prompt: Some("test prompt".to_string()),
        template: None,
        existing_image: None,
        image_model: Some("nano-banana".to_string()),
        model_3d: "trellis-2".to_string(),
        export_fbx: true,
    };

    let mut metadata = BundleMetadata::with_config(config);
    metadata.add_tag("test".to_string());

    // Save metadata
    let result = metadata.save(&bundle_dir);
    assert!(result.is_ok(), "Should save metadata: {:?}", result.err());

    // Verify file exists
    let metadata_path = bundle_dir.join(bundle_files::METADATA);
    assert!(metadata_path.exists(), "bundle.json should exist");

    // Load metadata
    let loaded = BundleMetadata::load(&bundle_dir)
        .expect("Should load metadata")
        .expect("Metadata should exist");

    // Verify contents
    assert!(loaded.config.is_some());
    let loaded_config = loaded.config.as_ref().unwrap();
    assert_eq!(loaded_config.prompt, Some("test prompt".to_string()));
    assert_eq!(loaded_config.image_model, Some("nano-banana".to_string()));
    assert_eq!(loaded_config.model_3d, "trellis-2");
    assert!(loaded.tags.contains(&"test".to_string()));
}

// =============================================================================
// Timestamp Generation Tests
// =============================================================================

#[test]
fn test_timestamp_format() {
    let timestamp = generate_timestamp();

    // Should be exactly 17 characters: YYYY-MM-DD_HHMMSS
    assert_eq!(timestamp.len(), 17, "Timestamp should be 17 chars");

    // Should contain exactly one underscore separating date and time
    let parts: Vec<&str> = timestamp.split('_').collect();
    assert_eq!(parts.len(), 2, "Should split into date and time");
    assert_eq!(
        parts[0].len(),
        10,
        "Date part should be 10 chars (YYYY-MM-DD)"
    );
    assert_eq!(parts[1].len(), 6, "Time part should be 6 chars (HHMMSS)");

    // Date part should be YYYY-MM-DD
    let date_parts: Vec<&str> = parts[0].split('-').collect();
    assert_eq!(date_parts.len(), 3, "Date should have 3 parts");
    assert_eq!(date_parts[0].len(), 4, "Year should be 4 digits");
    assert_eq!(date_parts[1].len(), 2, "Month should be 2 digits");
    assert_eq!(date_parts[2].len(), 2, "Day should be 2 digits");

    // Time part should be all digits
    assert!(parts[1].chars().all(|c| c.is_ascii_digit()));
}

#[test]
fn test_timestamp_uniqueness() {
    let mut timestamps = Vec::new();

    // Generate timestamps with delays to ensure uniqueness
    for _ in 0..3 {
        timestamps.push(generate_timestamp());
        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    // Verify uniqueness — timestamps generated 1 second apart should differ
    timestamps.dedup();
    assert_eq!(
        timestamps.len(),
        3,
        "Timestamps generated 1s apart should be unique"
    );
}

// =============================================================================
// File Permissions and Error Handling Tests
// =============================================================================

#[test]
#[cfg(unix)] // Unix-specific permission tests
fn test_readonly_directory_error() {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = TempDir::new().unwrap();
    let readonly_dir = temp_dir.path().join("readonly");

    // Create directory
    fs::create_dir(&readonly_dir).unwrap();

    // Make it read-only
    let mut perms = fs::metadata(&readonly_dir).unwrap().permissions();
    perms.set_mode(0o444); // Read-only
    fs::set_permissions(&readonly_dir, perms).unwrap();

    // Try to create a file in read-only directory
    let file_path = readonly_dir.join("test.txt");
    let result = fs::write(&file_path, "test");

    assert!(
        result.is_err(),
        "Writing to read-only directory should fail"
    );

    // Cleanup: restore write permissions so TempDir can clean up
    let mut perms = fs::metadata(&readonly_dir).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&readonly_dir, perms).unwrap();
}

#[test]
fn test_nonexistent_directory_error() {
    let temp_dir = TempDir::new().unwrap();
    let nonexistent = temp_dir.path().join("does-not-exist").join("file.txt");

    // Writing without creating parent should fail
    let result = fs::write(&nonexistent, "test");
    assert!(result.is_err(), "Should fail when parent doesn't exist");
}

// =============================================================================
// Bundle Discovery Tests
// =============================================================================

#[test]
fn test_list_bundle_directories() {
    let temp_dir = TempDir::new().unwrap();

    // Create multiple bundle directories
    let bundles = vec![
        "2024-01-01_120000",
        "2024-01-02_143000",
        "2024-01-03_090000",
    ];

    for bundle in &bundles {
        let bundle_dir = temp_dir.path().join(bundle);
        fs::create_dir_all(&bundle_dir).unwrap();
        // Create a marker file
        fs::write(bundle_dir.join(bundle_files::METADATA), "{}").unwrap();
    }

    // List directories
    let entries: Vec<_> = fs::read_dir(temp_dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();

    assert_eq!(entries.len(), bundles.len(), "Should find all bundles");
}

#[test]
fn test_filter_bundle_directories_by_timestamp_format() {
    let temp_dir = TempDir::new().unwrap();

    // Create valid and invalid directory names
    fs::create_dir_all(temp_dir.path().join("2024-01-01_120000")).unwrap(); // Valid
    fs::create_dir_all(temp_dir.path().join("2024-01-02_143000")).unwrap(); // Valid
    fs::create_dir_all(temp_dir.path().join("not_a_bundle")).unwrap(); // Invalid
    fs::create_dir_all(temp_dir.path().join("2024-01-01")).unwrap(); // Invalid (no time)

    let entries: Vec<_> = fs::read_dir(temp_dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            if let Some(name) = e.file_name().to_str() {
                // Check timestamp format: YYYY-MM-DD_HHMMSS
                name.len() == 17 && name.contains('_')
            } else {
                false
            }
        })
        .collect();

    assert_eq!(
        entries.len(),
        2,
        "Should only find valid timestamp directories"
    );
}
