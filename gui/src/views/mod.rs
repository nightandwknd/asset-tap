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

use eframe::egui;
use std::path::Path;

/// Behavior when the user clicks outside a modal backdrop.
pub enum BackdropClick {
    /// Clicking outside closes the modal.
    Close,
    /// Clicking outside closes the modal only if the condition is true.
    CloseIf(bool),
    /// Clicking outside does nothing (user must interact with the modal).
    Block,
}

/// Render a semi-transparent modal backdrop that covers the entire screen.
///
/// Returns `true` if the user clicked outside (and the backdrop is configured to close).
/// Use this before rendering the modal window itself.
pub fn modal_backdrop(ctx: &egui::Context, id: &str, alpha: u8, click: BackdropClick) -> bool {
    let mut clicked_outside = false;

    egui::Area::new(egui::Id::new(id))
        .fixed_pos(egui::pos2(0.0, 0.0))
        .order(egui::Order::Background)
        .show(ctx, |ui| {
            let screen_rect = ctx.content_rect();

            match click {
                BackdropClick::Close => {
                    if ui
                        .allocate_response(screen_rect.size(), egui::Sense::click())
                        .clicked()
                    {
                        clicked_outside = true;
                    }
                }
                BackdropClick::CloseIf(can_close) => {
                    if can_close {
                        if ui
                            .allocate_response(screen_rect.size(), egui::Sense::click())
                            .clicked()
                        {
                            clicked_outside = true;
                        }
                    } else {
                        ui.allocate_response(screen_rect.size(), egui::Sense::hover());
                    }
                }
                BackdropClick::Block => {
                    ui.allocate_response(screen_rect.size(), egui::Sense::hover());
                }
            }

            ui.painter()
                .rect_filled(screen_rect, 0.0, egui::Color32::from_black_alpha(alpha));
        });

    clicked_outside
}

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
