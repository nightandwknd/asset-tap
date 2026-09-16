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

/// Clip-pack download copy. The question is the confirm; hover is a statement.
/// Size lives only on the confirm.
pub mod clip_packs {
    use asset_tap_core::constants::files::CLIP_PACKS_SIZE_LABEL;

    pub const DOWNLOAD_ACTION: &str = "Download animation packs";
    pub const DOWNLOAD_BUSY: &str = "Downloading animation packs...";
    pub const DOWNLOAD_HOVER: &str = "Two animation packs by Quaternius.";
    pub const DOWNLOAD_PROMPT: &str = "Download two animation packs by Quaternius?";
    pub const SOURCE_HOVER: &str = "Quaternius page. Paid Source has extra clips.";
    pub const ADD_PACK_HOVER: &str = "Install or download an animation library.";
    pub const ARCHIVE_HOVER: &str = "A zip, .glb, or .gltf.";

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
