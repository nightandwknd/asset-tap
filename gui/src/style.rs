//! Application styling constants.
//!
//! Provides consistent colors and text styles throughout the app.

use eframe::egui::{Color32, RichText};

/// Secondary text color - brighter than egui's default weak() for better readability.
/// Used for hints, descriptions, and less prominent information.
pub const SECONDARY_TEXT: Color32 = Color32::from_gray(160);

/// Extension trait for RichText to apply our custom secondary style.
pub trait RichTextExt {
    /// Apply secondary text styling (like weak() but brighter).
    fn secondary(self) -> Self;
}

impl RichTextExt for RichText {
    fn secondary(self) -> Self {
        self.color(SECONDARY_TEXT)
    }
}

/// Create secondary-styled text directly.
pub fn secondary(text: impl Into<String>) -> RichText {
    RichText::new(text).color(SECONDARY_TEXT)
}
