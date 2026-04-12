//! Embedded config file sync for providers and templates.
//!
//! Compares embedded (compiled-in) config files byte-for-byte against their
//! on-disk counterparts and overwrites on-disk files when they differ. Old
//! files are backed up as `.yaml.bak` before overwriting.
//!
//! There is no version field or manual bumping: the content itself is the
//! version. Any byte-level change to a `providers/*.yaml` or `templates/*.yaml`
//! file is picked up automatically on the next app launch.
//!
//! Design note: this is only safe for files the app *owns* — embedded configs
//! that ship with the binary. User-created configs (different filenames, or
//! files without an embedded counterpart) are never touched by this module.

use anyhow::Result;
use std::path::Path;

/// Backup sidecar extension produced when an on-disk embedded config is
/// overwritten. Passed to [`Path::with_extension`], so it replaces the entire
/// existing extension — `provider.yaml` becomes `provider.yaml.bak`.
const BACKUP_EXT: &str = "yaml.bak";

/// Result of comparing an embedded config against its on-disk counterpart.
#[derive(Debug, PartialEq)]
pub enum SyncAction {
    /// File does not exist on disk — write it.
    WriteNew,
    /// On-disk bytes differ from embedded — overwrite (with backup).
    Overwrite,
    /// On-disk bytes match embedded — skip.
    UpToDate,
}

/// Determine what action to take for an embedded config file.
pub fn determine_action(embedded_content: &str, target_path: &Path) -> SyncAction {
    if !target_path.exists() {
        return SyncAction::WriteNew;
    }

    match std::fs::read_to_string(target_path) {
        Ok(on_disk) if on_disk == embedded_content => SyncAction::UpToDate,
        Ok(_) => SyncAction::Overwrite,
        // If we can't read the file for any reason, assume it needs rewriting.
        // The subsequent write will either succeed or surface the real error.
        Err(_) => SyncAction::Overwrite,
    }
}

/// Write an embedded config to disk, creating a backup if the file already exists
/// and its content differs from the embedded version.
///
/// Returns `Ok(true)` if the file was written, `Ok(false)` if skipped.
pub fn write_with_backup(
    target_path: &Path,
    embedded_content: &str,
    config_type: &str,
) -> Result<bool> {
    match determine_action(embedded_content, target_path) {
        SyncAction::WriteNew => {
            std::fs::write(target_path, embedded_content)?;
            tracing::info!("Created default {} config: {:?}", config_type, target_path);
            Ok(true)
        }
        SyncAction::Overwrite => {
            let backup_path = target_path.with_extension(BACKUP_EXT);
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
                "Updated {} config {:?} (content changed vs embedded)",
                config_type,
                target_path.file_name().unwrap_or_default(),
            );
            Ok(true)
        }
        SyncAction::UpToDate => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_determine_action_file_missing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nonexistent.yaml");
        assert_eq!(determine_action("id: test\n", &path), SyncAction::WriteNew);
    }

    #[test]
    fn test_determine_action_content_matches() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.yaml");
        let content = "id: test\nname: Test\n";
        fs::write(&path, content).unwrap();
        assert_eq!(determine_action(content, &path), SyncAction::UpToDate);
    }

    #[test]
    fn test_determine_action_content_differs() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.yaml");
        fs::write(&path, "id: old\n").unwrap();
        assert_eq!(determine_action("id: new\n", &path), SyncAction::Overwrite);
    }

    #[test]
    fn test_determine_action_whitespace_sensitive() {
        // A trailing newline difference is enough to trigger an overwrite —
        // this is intentional. "Same in spirit" isn't something we try to
        // judge; byte equality is the only truth.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.yaml");
        fs::write(&path, "id: test\n").unwrap();
        assert_eq!(
            determine_action("id: test\n\n", &path),
            SyncAction::Overwrite
        );
    }

    #[test]
    fn test_write_with_backup_new_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("new.yaml");
        let result = write_with_backup(&path, "id: new\n", "test").unwrap();
        assert!(result);
        assert!(path.exists());
        assert!(!path.with_extension(BACKUP_EXT).exists());
    }

    #[test]
    fn test_write_with_backup_overwrite_creates_backup() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.yaml");
        let old_content = "id: old\nname: Old\n";
        let new_content = "id: new\nname: New\n";
        fs::write(&path, old_content).unwrap();

        let result = write_with_backup(&path, new_content, "test").unwrap();
        assert!(result);
        assert_eq!(fs::read_to_string(&path).unwrap(), new_content);

        let backup = path.with_extension(BACKUP_EXT);
        assert!(backup.exists());
        assert_eq!(fs::read_to_string(&backup).unwrap(), old_content);
    }

    #[test]
    fn test_write_with_backup_skips_when_identical() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.yaml");
        let content = "id: test\n";
        fs::write(&path, content).unwrap();

        let result = write_with_backup(&path, content, "test").unwrap();
        assert!(!result);
        assert!(!path.with_extension(BACKUP_EXT).exists());
    }

    /// On Unix, if the on-disk file exists but can't be read (no permission),
    /// `determine_action` should classify it as `Overwrite` so the next write
    /// has a chance to either succeed or surface a real error. Returning
    /// `UpToDate` would be wrong: we have no idea what the bytes are, so we
    /// can't claim they match.
    #[cfg(unix)]
    #[test]
    fn test_determine_action_unreadable_file_treated_as_overwrite() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.yaml");
        fs::write(&path, "id: anything\n").unwrap();

        // Strip all permissions so read_to_string fails with EACCES.
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();

        let action = determine_action("id: embedded\n", &path);

        // Restore perms before assertions so a panic still leaves a cleanable
        // tempdir for tempfile to remove.
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        assert_eq!(action, SyncAction::Overwrite);
    }
}
