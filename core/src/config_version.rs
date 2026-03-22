//! Config file versioning for automatic upgrades.
//!
//! Compares embedded (compiled-in) config versions against on-disk versions
//! and overwrites on-disk files when the embedded version is newer.
//! Old files are backed up as `.yaml.bak` before overwriting.

use anyhow::Result;
use serde::Deserialize;
use std::path::Path;

/// Minimal struct for extracting just the config_version from any YAML file.
/// All other fields are ignored by serde (default behavior).
#[derive(Deserialize)]
struct ConfigVersionOnly {
    #[serde(default)]
    config_version: u32,
}

/// Result of comparing an embedded config against its on-disk counterpart.
#[derive(Debug, PartialEq)]
pub enum VersionAction {
    /// File does not exist on disk — write it.
    WriteNew,
    /// Embedded version is newer — overwrite (with backup).
    Upgrade { on_disk: u32, embedded: u32 },
    /// Versions are equal or on-disk is newer — skip.
    UpToDate,
}

/// Extract the config_version from a YAML string without full parsing.
///
/// Returns 0 if the field is missing or the YAML cannot be parsed.
pub fn extract_version(yaml_content: &str) -> u32 {
    serde_yaml_ng::from_str::<ConfigVersionOnly>(yaml_content)
        .map(|v| v.config_version)
        .unwrap_or(0)
}

/// Extract the config_version from an on-disk YAML file.
///
/// Returns 0 if the file cannot be read or parsed, or if the field is missing.
pub fn extract_version_from_file(path: &Path) -> u32 {
    std::fs::read_to_string(path)
        .ok()
        .map(|content| extract_version(&content))
        .unwrap_or(0)
}

/// Determine what action to take for an embedded config file.
pub fn determine_action(embedded_content: &str, target_path: &Path) -> VersionAction {
    if !target_path.exists() {
        return VersionAction::WriteNew;
    }

    let embedded_version = extract_version(embedded_content);
    let on_disk_version = extract_version_from_file(target_path);

    if embedded_version > on_disk_version {
        VersionAction::Upgrade {
            on_disk: on_disk_version,
            embedded: embedded_version,
        }
    } else {
        VersionAction::UpToDate
    }
}

/// Write an embedded config to disk, creating a backup if the file already exists.
///
/// Returns `Ok(true)` if the file was written, `Ok(false)` if skipped.
pub fn write_with_backup(
    target_path: &Path,
    embedded_content: &str,
    config_type: &str,
) -> Result<bool> {
    let action = determine_action(embedded_content, target_path);

    match action {
        VersionAction::WriteNew => {
            std::fs::write(target_path, embedded_content)?;
            tracing::info!("Created default {} config: {:?}", config_type, target_path);
            Ok(true)
        }
        VersionAction::Upgrade { on_disk, embedded } => {
            // Create backup before overwriting
            let backup_path = target_path.with_extension("yaml.bak");
            if let Err(e) = std::fs::copy(target_path, &backup_path) {
                tracing::warn!(
                    "Failed to backup {} config {:?}: {}",
                    config_type,
                    target_path,
                    e
                );
            } else {
                tracing::info!("Backed up {} config to {:?}", config_type, backup_path);
            }

            std::fs::write(target_path, embedded_content)?;
            tracing::info!(
                "Upgraded {} config {:?}: version {} -> {}",
                config_type,
                target_path.file_name().unwrap_or_default(),
                on_disk,
                embedded
            );
            Ok(true)
        }
        VersionAction::UpToDate => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_extract_version_present() {
        let yaml = "config_version: 5\nid: test\n";
        assert_eq!(extract_version(yaml), 5);
    }

    #[test]
    fn test_extract_version_missing() {
        let yaml = "id: test\nname: Test\n";
        assert_eq!(extract_version(yaml), 0);
    }

    #[test]
    fn test_extract_version_invalid_yaml() {
        let yaml = "not: valid: yaml: {{{}}}";
        assert_eq!(extract_version(yaml), 0);
    }

    #[test]
    fn test_extract_version_from_full_provider_yaml() {
        let yaml = concat!(
            "config_version: 3\n",
            "\n",
            "x-anchor: &anchor\n",
            "  key: value\n",
            "\n",
            "provider:\n",
            "  id: \"test\"\n",
            "  name: \"Test\"\n",
            "  description: \"Test provider\"\n",
            "  env_vars: [\"KEY\"]\n",
            "\n",
            "text_to_image: []\n",
        );
        assert_eq!(extract_version(yaml), 3);
    }

    #[test]
    fn test_determine_action_file_missing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nonexistent.yaml");
        assert_eq!(
            determine_action("config_version: 1\n", &path),
            VersionAction::WriteNew
        );
    }

    #[test]
    fn test_determine_action_embedded_newer() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.yaml");
        fs::write(&path, "config_version: 1\nid: test\n").unwrap();
        assert_eq!(
            determine_action("config_version: 2\nid: test\n", &path),
            VersionAction::Upgrade {
                on_disk: 1,
                embedded: 2
            }
        );
    }

    #[test]
    fn test_determine_action_same_version() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.yaml");
        fs::write(&path, "config_version: 3\nid: test\n").unwrap();
        assert_eq!(
            determine_action("config_version: 3\nid: test\n", &path),
            VersionAction::UpToDate
        );
    }

    #[test]
    fn test_determine_action_on_disk_missing_version_treated_as_zero() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.yaml");
        fs::write(&path, "id: test\nname: Test\n").unwrap();
        assert_eq!(
            determine_action("config_version: 1\nid: test\n", &path),
            VersionAction::Upgrade {
                on_disk: 0,
                embedded: 1
            }
        );
    }

    #[test]
    fn test_write_with_backup_new_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("new.yaml");
        let result = write_with_backup(&path, "config_version: 1\nid: new\n", "test").unwrap();
        assert!(result);
        assert!(path.exists());
        assert!(!path.with_extension("yaml.bak").exists());
    }

    #[test]
    fn test_write_with_backup_upgrade_creates_backup() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.yaml");
        let old_content = "config_version: 1\nid: old\n";
        let new_content = "config_version: 2\nid: new\n";
        fs::write(&path, old_content).unwrap();

        let result = write_with_backup(&path, new_content, "test").unwrap();
        assert!(result);
        assert_eq!(fs::read_to_string(&path).unwrap(), new_content);

        let backup = path.with_extension("yaml.bak");
        assert!(backup.exists());
        assert_eq!(fs::read_to_string(&backup).unwrap(), old_content);
    }

    #[test]
    fn test_write_with_backup_no_overwrite_when_current() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.yaml");
        let content = "config_version: 2\nid: test\n";
        fs::write(&path, content).unwrap();

        let result = write_with_backup(&path, content, "test").unwrap();
        assert!(!result);
        assert!(!path.with_extension("yaml.bak").exists());
    }
}
