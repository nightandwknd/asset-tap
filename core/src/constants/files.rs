//! File name constants.
//!
//! This module defines all standard file names used by Asset Tap.
//! These are used for configuration files, output files, and metadata.

/// Application identifier for OS-specific paths
pub const APP_NAME: &str = "asset-tap";

/// User-facing application display name
pub const APP_DISPLAY_NAME: &str = "Asset Tap";

/// Reverse-DNS application identifier (matches cargo-packager `identifier` in gui/Cargo.toml)
pub const APP_ID: &str = "com.nightandwknd.asset-tap";

/// Demo bundle directory name
pub const DEMO_DIR: &str = "_asset-tap";

/// Configuration files
pub mod config {
    /// Main settings file
    pub const SETTINGS: &str = "settings.json";

    /// Development mode settings file
    pub const DEV_SETTINGS: &str = ".dev/settings.json";

    /// Custom templates file (deprecated, migrated to YAML)
    pub const CUSTOM_TEMPLATES: &str = "custom_templates.json";

    /// Templates directory
    pub const TEMPLATES_DIR: &str = "templates";

    /// History file
    pub const HISTORY: &str = "history.json";
}

/// Output bundle files
pub mod bundle {
    /// Bundle metadata file
    pub const METADATA: &str = "bundle.json";

    /// Generated image file
    pub const IMAGE: &str = "image.png";

    /// 3D model file (GLB format)
    pub const MODEL_GLB: &str = "model.glb";

    /// 3D model file (FBX format)
    pub const MODEL_FBX: &str = "model.fbx";

    /// Textures directory
    pub const TEXTURES_DIR: &str = "textures";
}

/// Zip archive file names
pub mod archive {
    /// Textures zip archive
    pub const TEXTURES_ZIP: &str = "textures.zip";
}

/// Development directories
pub mod dev_dirs {
    /// Root development directory
    pub const ROOT: &str = ".dev";

    /// Development output directory
    pub const OUTPUT: &str = ".dev/output";

    /// Development providers directory
    pub const PROVIDERS: &str = ".dev/providers";

    /// Development templates directory
    pub const TEMPLATES: &str = ".dev/templates";

    /// Development logs directory
    pub const LOGS: &str = ".dev/logs";
}
