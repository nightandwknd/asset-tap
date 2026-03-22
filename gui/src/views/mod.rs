//! UI view components.

pub mod about;
pub mod bundle_info;
pub mod confirmation_dialog;
pub mod image_approval;
pub mod library;
pub mod preview;
pub mod progress;
pub mod settings;
pub mod sidebar;
pub mod template_editor;
pub mod walkthrough;
pub mod welcome_modal;

use std::path::Path;

/// Convert a file path to a properly formatted file:// URI.
///
/// Note: egui's image loader expects raw (non-percent-encoded) file URIs.
/// Spaces and other special characters are passed through as-is.
///
/// Handles cross-platform differences:
/// - Unix/macOS: `file:///path/to/file`
/// - Windows: `file:///C:/path/to/file`
pub fn path_to_file_uri(path: &Path) -> String {
    let path_str = path.display().to_string();

    #[cfg(target_os = "windows")]
    {
        // Windows paths need forward slashes and proper prefix
        // C:\Users\file.png -> file:///C:/Users/file.png
        let normalized = path_str.replace('\\', "/");
        format!("file:///{}", normalized)
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Unix paths already start with /, so file:// + /path = file:///path
        format!("file://{}", path_str)
    }
}
