//! File name constants.
//!
//! This module defines all standard file names used by Asset Tap.
//! These are used for configuration files, output files, and metadata.

/// Application identifier for OS-specific paths
pub const APP_NAME: &str = "asset-tap";

/// User-facing application display name
pub const APP_DISPLAY_NAME: &str = "Asset Tap";

/// One-line product category. The `--help` about line, the GUI About tagline,
/// and the cargo-packager `description` in `gui/Cargo.toml` all say this.
pub const APP_CATEGORY: &str = "Game asset generation pipeline";

/// Headline: what goes in and what comes out.
pub const APP_HERO: &str = "From prompt, image, or mesh to a rigged bundle.";

/// Subhead, beneath [`APP_HERO`].
pub const APP_SUBHEAD: &str = "Your machine, your providers, the whole asset lifecycle.";

/// Single-sentence description for package metadata and site copy.
pub const APP_ONE_LINER: &str = "Open-source game asset generation pipeline.";

/// Long-form description. Mirrored by the cargo-packager `long_description`
/// in `gui/Cargo.toml` (enforced by `cli/tests/positioning.rs`).
pub const APP_DESCRIPTION: &str = "Asset Tap turns a prompt, an image, or a mesh into a game-ready \
asset: concept art, a textured GLB, humanoid rigging with animation clips, all in one bundle. \
Desktop app, CLI, and MCP server. Open source, with generation routed to your chosen provider.";

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

/// Extensions accepted as a still image: pipeline input and loose bundle import.
pub const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "avif"];

/// True when `path` has an extension in [`IMAGE_EXTS`].
pub fn is_image_path(path: &std::path::Path) -> bool {
    path.extension()
        .is_some_and(|e| IMAGE_EXTS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// Make a user-supplied name safe to use as a file name stem.
///
/// Path separators, the characters Windows rejects (`: * ? " < > |`), NUL and
/// other control characters become `_`; surrounding whitespace and leading or
/// trailing dots are trimmed; an empty result becomes `asset`.
///
/// **Unicode letters are kept.** The rule is deliberately permissive: an
/// earlier alphanumeric-only filter in the GUI export path mangled non-ASCII
/// bundle names into rows of underscores. Everything modern filesystems accept
/// is allowed through; only what would traverse a directory or be rejected
/// outright is replaced.
///
/// Single source for `--install`'s directory form (CLI) and the export zip
/// name (GUI), so the two front doors cannot drift.
pub fn safe_filename_stem(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').trim().to_string();
    if cleaned.is_empty() {
        "asset".to_string()
    } else {
        cleaned
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn image_path_matches_pipeline_and_import_formats() {
        for ok in ["a.png", "b.JPG", "c.jpeg", "d.webp", "e.gif", "f.avif"] {
            assert!(is_image_path(Path::new(ok)), "{ok}");
        }
        for no in ["m.glb", "z.zip", "bundle.json", "noext"] {
            assert!(!is_image_path(Path::new(no)), "{no}");
        }
    }

    /// The CLI's `--install` directory form leans on this: a bundle name is
    /// free text and must not be able to steer the copy out of the directory
    /// the user named.
    #[test]
    fn safe_filename_stem_neutralizes_separators_and_reserved_chars() {
        assert_eq!(safe_filename_stem("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(safe_filename_stem("a\\b"), "a_b");
        assert_eq!(safe_filename_stem("a:b*c?d\"e<f>g|h"), "a_b_c_d_e_f_g_h");
        assert_eq!(safe_filename_stem("tab\there"), "tab_here");
    }

    #[test]
    fn safe_filename_stem_falls_back_when_nothing_is_left() {
        assert_eq!(safe_filename_stem("   "), "asset");
        assert_eq!(safe_filename_stem(""), "asset");
        assert_eq!(safe_filename_stem("..."), "asset");
    }

    /// Ordinary names, including the GUI export path's spaces and dashes,
    /// survive untouched.
    #[test]
    fn safe_filename_stem_keeps_ordinary_names() {
        assert_eq!(safe_filename_stem("My Robot-01_v2"), "My Robot-01_v2");
        assert_eq!(safe_filename_stem("  padded  "), "padded");
    }

    /// The GUI's old alphanumeric-only filter turned these into underscores.
    #[test]
    fn safe_filename_stem_keeps_unicode_letters() {
        assert_eq!(safe_filename_stem("剣士"), "剣士");
        assert_eq!(safe_filename_stem("café"), "café");
    }
}
