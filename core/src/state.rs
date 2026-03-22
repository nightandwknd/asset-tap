//! Application state persistence.
//!
//! Tracks the current application state for session recovery:
//! - Currently viewed generation
//! - UI state (selected tab, scroll positions, etc.)
//! - In-progress generation (for crash recovery)
//!
//! State is stored separately from settings to allow frequent updates
//! without risking settings corruption.
//!
//! ## File Locations
//!
//! - **Dev mode**: `./.dev/state.json`
//! - **Release mode**: OS-specific config directory alongside `settings.json`

use crate::constants::files::dev_dirs;
use crate::settings::is_dev_mode;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// State filename.
const STATE_FILE: &str = "state.json";

/// Model metadata for persistence.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelInfo {
    pub file_size: u64,
    pub format: String,
    pub vertex_count: usize,
    pub triangle_count: usize,
}

/// A single prompt history entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptHistoryEntry {
    /// The prompt text.
    pub prompt: String,

    /// The template used (if any).
    pub template: Option<String>,
}

/// Application state for session recovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppState {
    /// Currently viewed generation directory (if any).
    pub current_generation: Option<PathBuf>,

    /// Currently selected preview tab.
    pub preview_tab: String,

    /// Whether the sidebar is collapsed.
    pub sidebar_collapsed: bool,

    /// Last used prompt (for convenience).
    /// Note: This is NOT restored on app startup to give a fresh slate.
    pub last_prompt: Option<String>,

    /// Prompt history (most recent first, max 20).
    pub prompt_history: Vec<PromptHistoryEntry>,

    /// In-progress generation ID (for crash recovery).
    /// If this is set on startup, the generation may have been interrupted.
    pub in_progress_generation: Option<String>,

    /// Window position and size (for restore).
    pub window_state: Option<WindowState>,

    /// Cached model info for the current generation.
    pub model_info: Option<ModelInfo>,

    /// Whether to show welcome screen on startup.
    pub show_welcome_on_startup: bool,

    /// Whether the user has completed (or dismissed) the interactive walkthrough.
    pub has_completed_walkthrough: bool,

    /// Whether to show confirmation dialog when loading associated assets.
    pub show_associated_assets_dialog: bool,

    /// Last selected image provider.
    pub selected_image_provider: Option<String>,

    /// Last selected image model.
    pub selected_image_model: Option<String>,

    /// Last selected 3D provider.
    pub selected_3d_provider: Option<String>,

    /// Last selected 3D model.
    pub selected_3d_model: Option<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            current_generation: None,
            preview_tab: String::new(),
            sidebar_collapsed: false,
            last_prompt: None,
            prompt_history: Vec::new(),
            in_progress_generation: None,
            window_state: None,
            model_info: None,
            show_welcome_on_startup: true,
            has_completed_walkthrough: false,
            show_associated_assets_dialog: true,
            selected_image_provider: None,
            selected_image_model: None,
            selected_3d_provider: None,
            selected_3d_model: None,
        }
    }
}

/// Window position and size state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowState {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

impl AppState {
    /// Load state from the state file.
    ///
    /// Returns default state if file doesn't exist or is invalid.
    pub fn load() -> Self {
        let path = state_file_path();

        if !path.exists() {
            return Self::default();
        }

        match std::fs::read_to_string(&path) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Save state to the state file.
    pub fn save(&self) -> std::io::Result<()> {
        let path = state_file_path();

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let contents = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;

        std::fs::write(&path, contents)
    }

    /// Mark a generation as in-progress (for crash recovery).
    pub fn start_generation(&mut self, generation_id: &str) {
        self.in_progress_generation = Some(generation_id.to_string());
        let _ = self.save();
    }

    /// Clear the in-progress generation (completed or cancelled).
    pub fn finish_generation(&mut self) {
        self.in_progress_generation = None;
        let _ = self.save();
    }

    /// Set the currently viewed generation.
    pub fn set_current_generation(&mut self, path: Option<PathBuf>) {
        self.current_generation = path;
        let _ = self.save();
    }

    /// Check if there was an interrupted generation on startup.
    pub fn has_interrupted_generation(&self) -> bool {
        self.in_progress_generation.is_some()
    }
}

/// Get the path to the state file.
///
/// - Dev mode: `./.dev/state.json`
/// - Release mode: OS-specific config directory
pub fn state_file_path() -> PathBuf {
    if is_dev_mode() {
        PathBuf::from(dev_dirs::ROOT).join(STATE_FILE)
    } else {
        crate::settings::config_dir().join(STATE_FILE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_state() {
        let state = AppState::default();
        assert!(state.current_generation.is_none());
        assert!(state.in_progress_generation.is_none());
        assert!(!state.sidebar_collapsed);
        assert!(state.last_prompt.is_none());
        assert!(state.window_state.is_none());
        assert!(state.model_info.is_none());
        assert!(state.show_welcome_on_startup);
        assert!(!state.has_completed_walkthrough);
        assert!(state.show_associated_assets_dialog);
    }

    #[test]
    fn test_state_serialization() {
        let state = AppState {
            current_generation: Some(PathBuf::from("output/20241229_153045")),
            preview_tab: "Model3D".to_string(),
            ..Default::default()
        };

        let json = serde_json::to_string(&state).unwrap();
        let loaded: AppState = serde_json::from_str(&json).unwrap();

        assert_eq!(state.current_generation, loaded.current_generation);
        assert_eq!(state.preview_tab, loaded.preview_tab);
    }

    #[test]
    fn test_state_with_all_fields() {
        let state = AppState {
            current_generation: Some(PathBuf::from("/output/gen1")),
            preview_tab: "Image".to_string(),
            sidebar_collapsed: true,
            last_prompt: Some("a cowboy ninja with a leather duster, bandana mask, and dual katanas on the back".to_string()),
            prompt_history: vec![
                PromptHistoryEntry {
                    prompt: "a cowboy ninja with a leather duster, bandana mask, and dual katanas on the back".to_string(),
                    template: Some("character".to_string()),
                },
                PromptHistoryEntry {
                    prompt: "a cool car".to_string(),
                    template: None,
                },
            ],
            in_progress_generation: Some("gen_12345".to_string()),
            window_state: Some(WindowState {
                x: 100,
                y: 200,
                width: 1024,
                height: 768,
                maximized: false,
            }),
            model_info: Some(ModelInfo {
                file_size: 1024 * 1024,
                format: "GLB".to_string(),
                vertex_count: 5000,
                triangle_count: 10000,
            }),
            show_welcome_on_startup: false,
            has_completed_walkthrough: true,
            show_associated_assets_dialog: false,
            selected_image_provider: Some("fal.ai".to_string()),
            selected_image_model: Some("nano-banana-2".to_string()),
            selected_3d_provider: Some("fal.ai".to_string()),
            selected_3d_model: Some("trellis-2".to_string()),
        };

        let json = serde_json::to_string(&state).unwrap();
        let loaded: AppState = serde_json::from_str(&json).unwrap();

        assert!(loaded.sidebar_collapsed);
        assert_eq!(
            loaded.last_prompt,
            Some(
                "a cowboy ninja with a leather duster, bandana mask, and dual katanas on the back"
                    .to_string()
            )
        );
        assert_eq!(loaded.in_progress_generation, Some("gen_12345".to_string()));
        assert!(loaded.window_state.is_some());

        let ws = loaded.window_state.unwrap();
        assert_eq!(ws.x, 100);
        assert_eq!(ws.y, 200);
        assert_eq!(ws.width, 1024);
        assert_eq!(ws.height, 768);
        assert!(!ws.maximized);

        let mi = loaded.model_info.unwrap();
        assert_eq!(mi.file_size, 1024 * 1024);
        assert_eq!(mi.format, "GLB");
        assert_eq!(mi.vertex_count, 5000);
        assert_eq!(mi.triangle_count, 10000);

        assert!(!loaded.show_welcome_on_startup);
        assert!(loaded.has_completed_walkthrough);
        assert!(!loaded.show_associated_assets_dialog);
    }

    #[test]
    fn test_has_interrupted_generation() {
        let mut state = AppState::default();
        assert!(!state.has_interrupted_generation());

        state.in_progress_generation = Some("test_gen".to_string());
        assert!(state.has_interrupted_generation());
    }

    #[test]
    fn test_model_info_default() {
        let info = ModelInfo::default();
        assert_eq!(info.file_size, 0);
        assert!(info.format.is_empty());
        assert_eq!(info.vertex_count, 0);
        assert_eq!(info.triangle_count, 0);
    }

    #[test]
    fn test_window_state_serialization() {
        let ws = WindowState {
            x: -50,
            y: 100,
            width: 800,
            height: 600,
            maximized: true,
        };

        let json = serde_json::to_string(&ws).unwrap();
        let loaded: WindowState = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.x, -50);
        assert_eq!(loaded.y, 100);
        assert_eq!(loaded.width, 800);
        assert_eq!(loaded.height, 600);
        assert!(loaded.maximized);
    }

    #[test]
    fn test_state_deserialize_with_missing_fields() {
        // Ensure serde(default) works properly - missing fields get defaults
        let json = r#"{"preview_tab": "custom_tab"}"#;
        let loaded: AppState = serde_json::from_str(json).unwrap();

        assert!(loaded.current_generation.is_none());
        assert_eq!(loaded.preview_tab, "custom_tab");
        assert!(!loaded.sidebar_collapsed);
        // New fields default to true when missing from old state files
        assert!(loaded.show_welcome_on_startup);
        assert!(loaded.show_associated_assets_dialog);
        assert!(!loaded.has_completed_walkthrough);
    }
}
