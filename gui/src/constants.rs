//! UI constants for the asset-tap GUI.
//!
//! This module provides centralized constants for spacing, timing, copy, and
//! other UI-related values used throughout the GUI.

/// Spacing constants for UI layout
pub mod spacing {
    /// Large spacing between UI elements (8.0 pixels)
    pub const LARGE: f32 = 8.0;

    /// Small spacing between UI elements (4.0 pixels)
    pub const SMALL: f32 = 4.0;
}

/// Timing constants for UI animations and effects
pub mod timing {
    /// Toast fade-out duration in seconds
    pub const TOAST_FADE_OUT_DURATION: f32 = 0.5;

    /// Toast fade-in duration in seconds
    pub const TOAST_FADE_IN_DURATION: f32 = 0.2;
}

/// Clip-pack download copy.
///
/// Family name is always plural ("Libraries"). "1 + 2" names both packs.
/// `[Standard]` is the GitHub download; `[Source]` is the paid Quaternius tier.
/// The question is the confirm; hover is a statement. Size lives only on the
/// confirm.
pub mod clip_packs {
    use asset_tap_core::constants::files::CLIP_PACKS_SIZE_LABEL;

    pub const DOWNLOAD_ACTION: &str = "Download Universal Animation Libraries...";
    pub const DOWNLOAD_BUSY: &str = "Downloading Universal Animation Libraries [Standard]...";
    pub const DOWNLOAD_HOVER: &str = "Universal Animation Libraries 1 + 2 [Standard].";
    pub const DOWNLOAD_PROMPT: &str = "Download Universal Animation Libraries 1 + 2 [Standard]?";
    pub const SOURCE_HOVER: &str = "Quaternius site. [Source] packs have additional clips.";
    pub const ADD_PACK_HOVER: &str = "Install or download Universal Animation Libraries.";
    pub const ARCHIVE_HOVER: &str = "A zip, .glb, or .gltf.";
    pub const DROP_IMPORT: &str = "Drop to import";
    pub const DROP_INSTALL_PACK: &str = "Drop to install animation pack";
    pub const DROP_ATTACH_IMAGE: &str = "Drop to add image";
    pub const DROP_ATTACH_MODEL: &str = "Drop to add model";
    pub const DROP_PACKS_ON_ANIMATE: &str = "Drop packs on the Animation panel";
    pub const DROP_BUNDLES_ON_INFO: &str = "Drop on Bundle Info to import";
    pub const DROP_REPLACE_INPUT: &str = "Drop to replace the input image";
    pub const IMPORTING: &str = "Importing...";
    pub const INSTALLING_PACK: &str = "Installing animation pack...";
    pub const ATTACHING: &str = "Adding to bundle...";

    pub fn download_detail() -> String {
        format!("About {CLIP_PACKS_SIZE_LABEL}. Packs you already have are not replaced.")
    }
}

#[cfg(test)]
mod tests {
    use super::clip_packs;

    #[test]
    fn download_hover_is_a_statement() {
        assert!(
            !clip_packs::DOWNLOAD_HOVER.contains('?'),
            "question format is for the confirm"
        );
        assert!(
            clip_packs::DOWNLOAD_PROMPT.contains('?'),
            "confirm prompt must ask"
        );
        for s in [
            clip_packs::DOWNLOAD_ACTION,
            clip_packs::DOWNLOAD_BUSY,
            clip_packs::DOWNLOAD_HOVER,
            clip_packs::DOWNLOAD_PROMPT,
            clip_packs::SOURCE_HOVER,
            clip_packs::ADD_PACK_HOVER,
            clip_packs::ARCHIVE_HOVER,
            clip_packs::DROP_IMPORT,
            clip_packs::DROP_INSTALL_PACK,
            clip_packs::DROP_ATTACH_IMAGE,
            clip_packs::DROP_ATTACH_MODEL,
            clip_packs::DROP_PACKS_ON_ANIMATE,
            clip_packs::DROP_BUNDLES_ON_INFO,
            clip_packs::DROP_REPLACE_INPUT,
            clip_packs::IMPORTING,
            clip_packs::INSTALLING_PACK,
            clip_packs::ATTACHING,
        ] {
            assert!(
                !s.to_ascii_lowercase().contains("free"),
                "{s:?} still says free"
            );
        }
    }
}

/// Asset type identifiers for internal dispatch
pub mod asset_type {
    pub const IMAGE: &str = "image";
    pub const MODEL: &str = "model";
    pub const TEXTURES: &str = "textures";
    pub const ASSET: &str = "asset";
}

/// Callback identifiers for library browser file dialogs
pub mod callback {
    pub const EXISTING_IMAGE: &str = "existing_image";
    pub const PREVIEW_IMAGE: &str = "preview_image";
    pub const PREVIEW_MODEL: &str = "preview_model";
    pub const PREVIEW_TEXTURES: &str = "preview_textures";
}
