//! Confirmation dialog for loading associated assets.
//!
//! Shows a modal dialog when loading an asset that has associated files
//! in other tabs (image, model, textures). Allows users to confirm and
//! optionally disable future prompts.

use crate::icons;
use eframe::egui;

/// Associated assets that will be loaded alongside the selected asset.
#[derive(Debug, Clone)]
pub struct AssociatedAssets {
    /// Whether the bundle has an associated image.
    pub has_image: bool,
    /// Whether the bundle has an associated model.
    pub has_model: bool,
    /// Whether the bundle has associated textures.
    pub has_textures: bool,
}

impl AssociatedAssets {
    /// Check if there are any associated assets.
    pub fn has_any(&self) -> bool {
        self.has_image || self.has_model || self.has_textures
    }

    /// Get a human-readable list of associated assets.
    pub fn list(&self) -> Vec<&'static str> {
        let mut items = Vec::new();
        if self.has_image {
            items.push("Image");
        }
        if self.has_model {
            items.push("3D Model");
        }
        if self.has_textures {
            items.push("Textures");
        }
        items
    }

    /// Get a formatted string of associated assets.
    pub fn formatted(&self) -> String {
        let items = self.list();
        match items.len() {
            0 => String::new(),
            1 => items[0].to_string(),
            2 => format!("{} and {}", items[0], items[1]),
            _ => {
                let last = items.last().unwrap();
                let rest = &items[..items.len() - 1];
                format!("{}, and {}", rest.join(", "), last)
            }
        }
    }
}

/// Confirmation dialog state.
pub struct ConfirmationDialog {
    /// Whether the dialog is currently open.
    pub is_open: bool,
    /// Associated assets that will be loaded.
    pub associated_assets: Option<AssociatedAssets>,
    /// Asset type being selected (for display).
    pub selected_asset_type: String,
    /// Whether the "don't show again" checkbox is checked.
    dont_show_again: bool,
}

impl Default for ConfirmationDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfirmationDialog {
    /// Create a new confirmation dialog.
    pub fn new() -> Self {
        Self {
            is_open: false,
            associated_assets: None,
            selected_asset_type: String::new(),
            dont_show_again: false,
        }
    }

    /// Open the dialog with the specified associated assets.
    pub fn open(&mut self, selected_asset_type: &str, associated_assets: AssociatedAssets) {
        self.is_open = true;
        self.selected_asset_type = selected_asset_type.to_string();
        self.associated_assets = Some(associated_assets);
        self.dont_show_again = false;
    }

    /// Close the dialog.
    pub fn close(&mut self) {
        self.is_open = false;
        self.associated_assets = None;
        self.selected_asset_type.clear();
        self.dont_show_again = false;
    }

    /// Render the confirmation dialog.
    ///
    /// Returns:
    /// - `Some(true)` if user confirmed (load all assets)
    /// - `Some(false)` if user cancelled
    /// - `None` if dialog is not open or still waiting for input
    ///
    /// The `dont_show_again` flag is also returned if the user confirmed.
    pub fn render(&mut self, ctx: &egui::Context) -> (Option<bool>, bool) {
        if !self.is_open {
            return (None, false);
        }

        let mut result = None;
        let mut should_close = false;

        // Semi-transparent backdrop
        let modal_id = egui::Id::new("confirmation_dialog");
        egui::Area::new(modal_id.with("backdrop"))
            .fixed_pos(egui::pos2(0.0, 0.0))
            .order(egui::Order::Background)
            .show(ctx, |ui| {
                let screen_rect = ctx.content_rect();
                // Don't allow click-outside to close - user must explicitly confirm or cancel
                ui.allocate_response(screen_rect.size(), egui::Sense::hover());
                // Draw semi-transparent backdrop (darker for better contrast)
                ui.painter()
                    .rect_filled(screen_rect, 0, egui::Color32::from_black_alpha(200));
            });

        // Dialog window
        egui::Window::new("Load Associated Assets")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_width(450.0);

                // Icon and message
                ui.vertical_centered(|ui| {
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(icons::INFO)
                            .size(40.0)
                            .color(egui::Color32::from_rgb(100, 150, 255)),
                    );
                    ui.add_space(10.0);
                });

                // Main message
                if let Some(ref assets) = self.associated_assets {
                    let assets_list = assets.formatted();
                    ui.label(
                        egui::RichText::new(format!(
                            "Loading this {} will also populate the following tabs:",
                            self.selected_asset_type
                        ))
                        .size(14.0),
                    );

                    ui.add_space(8.0);

                    // Show list of associated assets
                    ui.indent("asset_list", |ui| {
                        ui.label(
                            egui::RichText::new(format!("• {}", assets_list))
                                .size(13.0)
                                .strong(),
                        );
                    });

                    ui.add_space(12.0);

                    ui.label(
                        egui::RichText::new("This makes it easy to view all related assets from this generation bundle.")
                            .size(12.0)
                            .weak(),
                    );
                }

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(8.0);

                // "Don't show again" checkbox
                ui.checkbox(&mut self.dont_show_again, "Don't show this message again");

                ui.add_space(12.0);

                // Buttons
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Load All button (primary action)
                        if ui
                            .button(
                                egui::RichText::new(format!("{} Load All", icons::CHECK))
                                    .size(14.0),
                            )
                            .clicked()
                        {
                            result = Some(true);
                            should_close = true;
                        }

                        // Cancel button
                        if ui.button(
                            egui::RichText::new(format!("{} Cancel", icons::X))
                                .size(14.0)
                        ).clicked() {
                            result = Some(false);
                            should_close = true;
                        }
                    });
                });

                ui.add_space(8.0);
            });

        // Handle escape key to close
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            result = Some(false);
            should_close = true;
        }

        // Handle enter key to confirm
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            result = Some(true);
            should_close = true;
        }

        let dont_show = self.dont_show_again;
        if should_close {
            self.close();
        }

        (result, dont_show)
    }
}
