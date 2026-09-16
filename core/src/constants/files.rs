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

/// Latest-release download prefix. Artifact names hang off this.
pub const GITHUB_RELEASES_LATEST: &str =
    "https://github.com/nightandwknd/asset-tap/releases/latest/download";

macro_rules! latest_asset {
    ($name:literal) => {
        concat!(
            "https://github.com/nightandwknd/asset-tap/releases/latest/download",
            $name
        )
    };
}

/// URL for the demo bundle manifest (small JSON with version info).
pub const DEMO_MANIFEST_URL: &str = latest_asset!("/demo-manifest.json");

/// URL for downloading the demo bundle archive from GitHub Releases.
pub const DEMO_BUNDLE_URL: &str = latest_asset!("/demo-bundle.zip");

/// Approximate size of the demo bundle download, shown in the UI.
pub const DEMO_BUNDLE_SIZE_LABEL: &str = "34 MB";

/// URL for the free Standard clip-pack manifest (version + SHA-256).
pub const CLIP_PACKS_MANIFEST_URL: &str = latest_asset!("/clip-packs-manifest.json");

/// URL for the free Standard clip-pack archive from GitHub Releases.
pub const CLIP_PACKS_URL: &str = latest_asset!("/clip-packs.zip");

/// Approximate size of the clip-pack archive, shown in the UI.
/// Must match `size_label` in `packs/manifest.json` (enforced by test).
pub const CLIP_PACKS_SIZE_LABEL: &str = "15 MB";

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
