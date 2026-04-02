//! Welcome modal for first-time setup.
//!
//! Shows a modal dialog on first launch to:
//! - Welcome the user
//! - Set up required configuration (output directory)
//! - Display release notes/changelog (future)
//! - Allow dismissing for returning users

use crate::icons;
use crate::style::RichTextExt;
use asset_tap_core::constants::files::APP_DISPLAY_NAME;
use eframe::egui;
use std::path::PathBuf;

/// Welcome modal state.
pub struct WelcomeModal {
    /// Whether the modal is currently open.
    is_open: bool,
    /// Selected output directory.
    output_dir: PathBuf,
    /// Whether to show welcome on startup (checked by default, like VS Code).
    show_on_startup: bool,
    /// Validation error message.
    error_message: Option<String>,
    /// Original values (to restore on cancel/close without save).
    original_output_dir: PathBuf,
    original_show_on_startup: bool,
}

impl WelcomeModal {
    /// Create a new welcome modal.
    pub fn new(output_dir: PathBuf) -> Self {
        Self {
            is_open: false,
            original_output_dir: output_dir.clone(),
            output_dir,
            original_show_on_startup: true,
            show_on_startup: true,
            error_message: None,
        }
    }

    /// Open the welcome modal.
    pub fn open(&mut self) {
        self.is_open = true;
        // Reset to original values when opening
        self.output_dir = self.original_output_dir.clone();
        self.show_on_startup = self.original_show_on_startup;
        self.error_message = None;
    }

    /// Render the welcome modal.
    ///
    /// Returns:
    /// - `Some((output_dir, show_on_startup, open_settings))` if user clicked "Get Started" or "Settings" link
    /// - `None` otherwise
    ///
    /// If `open_settings` is true, the settings modal should be opened.
    /// The welcome modal may remain open when settings is opened.
    ///
    /// `skip_backdrop` - If true, don't draw the backdrop (used when another modal is on top)
    /// `logo_texture` - Optional app logo texture to display
    pub fn render(
        &mut self,
        ctx: &egui::Context,
        skip_backdrop: bool,
        logo_texture: Option<&egui::TextureHandle>,
    ) -> Option<(PathBuf, bool, bool)> {
        if !self.is_open {
            return None;
        }

        let mut result = None;
        let mut should_close = false;

        // Semi-transparent backdrop (skip if another modal is on top to avoid double backdrop)
        if !skip_backdrop {
            let can_close = !self.output_dir.as_os_str().is_empty();
            if super::modal_backdrop(
                ctx,
                "welcome_backdrop",
                200,
                super::BackdropClick::CloseIf(can_close),
            ) {
                should_close = true;
            }
        }

        // Modal window
        egui::Window::new("Welcome")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_width(500.0);

                // Header with logo
                ui.vertical_centered(|ui| {
                    ui.add_space(10.0);

                    // Show logo if available, otherwise fallback to emoji
                    if let Some(logo) = logo_texture {
                        ui.image((logo.id(), egui::vec2(128.0, 128.0)));
                    } else {
                        ui.label(egui::RichText::new("👋").size(48.0));
                    }

                    ui.add_space(8.0);
                    ui.heading(
                        egui::RichText::new(format!("Welcome to {APP_DISPLAY_NAME}")).size(20.0),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("Generate 3D models from text prompts")
                            .size(14.0)
                            .secondary(),
                    );
                    ui.add_space(16.0);
                });

                ui.separator();
                ui.add_space(12.0);

                // Setup section
                ui.label(egui::RichText::new("Setup Required").size(15.0).strong());
                ui.add_space(8.0);

                ui.label("Choose where to save generated assets:");
                ui.add_space(6.0);

                // Output directory selector
                ui.horizontal(|ui| {
                    let mut path_text = self.output_dir.display().to_string();
                    let text_edit = egui::TextEdit::singleline(&mut path_text)
                        .desired_width(340.0)
                        .font(egui::TextStyle::Monospace);

                    if ui.add(text_edit).changed() {
                        self.output_dir = PathBuf::from(&path_text);
                    }

                    if ui.button(format!("{} Browse", icons::FOLDER)).clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .set_directory(&self.output_dir)
                            .pick_folder()
                    {
                        self.output_dir = path;
                    }
                });

                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(
                        "Each generation creates a timestamped folder with all related assets.",
                    )
                    .size(11.0)
                    .secondary()
                    .italics(),
                );

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(16.0);

                // Provider configuration callout
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(icons::INFO)
                            .size(18.0)
                            .color(egui::Color32::from_rgb(100, 180, 255)),
                    );
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("Configure API Keys")
                                .size(14.0)
                                .strong(),
                        );
                        ui.add_space(2.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("Set up provider API keys in")
                                    .size(12.0)
                                    .secondary(),
                            );
                            if ui
                                .link(egui::RichText::new("Settings").size(12.0))
                                .on_hover_text("Open Settings to configure API keys")
                                .clicked()
                            {
                                // Validate and save current values before opening settings
                                if self.output_dir.as_os_str().is_empty() {
                                    self.error_message = Some(
                                        "Output directory is required before opening Settings"
                                            .to_string(),
                                    );
                                } else {
                                    // Update original values but keep modal open
                                    self.original_output_dir = self.output_dir.clone();
                                    self.original_show_on_startup = self.show_on_startup;
                                    // Signal to open settings (welcome stays open in background)
                                    result =
                                        Some((self.output_dir.clone(), self.show_on_startup, true));
                                }
                            }
                            ui.label(
                                egui::RichText::new("to generate assets.")
                                    .size(12.0)
                                    .secondary(),
                            );
                        });
                        ui.add_space(2.0);
                        ui.label(
                            egui::RichText::new("You can explore the app without API keys.")
                                .size(11.0)
                                .secondary()
                                .italics(),
                        );
                    });
                });

                ui.add_space(20.0);

                // Info section
                ui.label(egui::RichText::new("What you can do:").size(15.0).strong());
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("•").size(14.0));
                    ui.label("Generate images from text prompts");
                });
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("•").size(14.0));
                    ui.label("Convert images to 3D models");
                });
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("•").size(14.0));
                    ui.label("Preview and export in multiple formats");
                });

                ui.add_space(20.0);
                ui.separator();
                ui.add_space(12.0);

                // Show on startup checkbox (checked by default, like VS Code)
                ui.checkbox(&mut self.show_on_startup, "Show welcome on startup");
                ui.add_space(8.0);

                // Error message
                if let Some(ref error) = self.error_message {
                    ui.label(
                        egui::RichText::new(error)
                            .color(egui::Color32::from_rgb(255, 100, 100))
                            .size(12.0),
                    );
                    ui.add_space(4.0);
                }

                // Buttons
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Get Started button (primary action)
                        if ui
                            .button(
                                egui::RichText::new(format!("{} Get Started", icons::CHECK))
                                    .size(14.0),
                            )
                            .clicked()
                        {
                            // Validate required field
                            if self.output_dir.as_os_str().is_empty() {
                                self.error_message =
                                    Some("Output directory is required".to_string());
                            } else {
                                self.error_message = None;
                                // Update original values on successful save
                                self.original_output_dir = self.output_dir.clone();
                                self.original_show_on_startup = self.show_on_startup;
                                result = Some((
                                    self.output_dir.clone(),
                                    self.show_on_startup,
                                    false, // Not opening settings when clicking "Get Started"
                                ));
                                should_close = true;
                            }
                        }
                    });
                });

                ui.add_space(8.0);
            });

        // Handle closing and return result
        if should_close {
            if result.is_none() {
                // Backdrop click — close modal and return current values so the
                // app runs the same initialization (bundle list refresh, etc.)
                self.is_open = false;
                result = Some((self.output_dir.clone(), self.show_on_startup, false));
            } else {
                // User clicked "Get Started" - close the modal
                self.is_open = false;
            }
        }

        // Return result if we have one (either from "Get Started", backdrop, or "Settings" link)
        result
    }
}
