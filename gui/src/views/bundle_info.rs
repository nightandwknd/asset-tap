//! Bundle information panel.
//!
//! Displays and allows editing of the current bundle's metadata:
//! - Custom name (editable)
//! - Prompt (read-only with copy to input button)
//! - Creation timestamp
//! - Model information

use crate::icons;
use asset_tap_core::bundle::Bundle;
use asset_tap_core::constants::files::bundle as bundle_files;
use eframe::egui;
use std::path::{Path, PathBuf};

/// Actions the bundle info panel can request from the app.
pub enum BundleInfoAction {
    /// User clicked "Copy to Input" — contains the prompt string.
    CopyPrompt(String),
    /// User selected a different bundle from the dropdown.
    SwitchBundle(PathBuf),
    /// User wants to export the bundle — contains (bundle_dir, destination_path).
    ExportBundle(PathBuf, PathBuf),
    /// User clicked the refresh button to rescan bundles from disk.
    RefreshList,
}

/// Bundle info panel state.
pub struct BundleInfoPanel {
    /// Current bundle being viewed.
    pub current_bundle: Option<Bundle>,
    /// Temporary name being edited (None = not editing).
    editing_name: Option<String>,
    /// Whether the name field has focus.
    name_has_focus: bool,
    /// Timestamp when the name was last saved (for showing "Saved!" indicator).
    last_save_time: Option<std::time::Instant>,
    /// Available bundles: (directory path, display name).
    available_bundles: Vec<(PathBuf, String)>,
}

impl Default for BundleInfoPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl BundleInfoPanel {
    /// Create a new bundle info panel.
    pub fn new() -> Self {
        Self {
            current_bundle: None,
            editing_name: None,
            name_has_focus: false,
            last_save_time: None,
            available_bundles: Vec::new(),
        }
    }

    /// Load a bundle into the panel.
    pub fn load_bundle(&mut self, bundle_path: PathBuf) -> Result<(), String> {
        match asset_tap_core::bundle::load_bundle(&bundle_path) {
            Ok(bundle) => {
                self.current_bundle = Some(bundle);
                self.editing_name = None;
                Ok(())
            }
            Err(e) => Err(format!("Failed to load bundle: {}", e)),
        }
    }

    /// Refresh the list of available bundles from the output directory.
    pub fn refresh_bundle_list(&mut self, output_dir: &Path) {
        let mut bundles = Vec::new();

        if let Ok(entries) = std::fs::read_dir(output_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }

                let dir_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();

                // Skip hidden directories and non-bundle dirs
                if dir_name.starts_with('.') {
                    continue;
                }

                // Try to read custom name from bundle.json
                let bundle_json = path.join(bundle_files::METADATA);
                let display_name = if bundle_json.exists() {
                    std::fs::read_to_string(&bundle_json)
                        .ok()
                        .and_then(|contents| {
                            serde_json::from_str::<serde_json::Value>(&contents).ok()
                        })
                        .and_then(|json| json.get("name")?.as_str().map(|s| s.to_string()))
                        .filter(|name| !name.is_empty())
                        .unwrap_or_else(|| dir_name.clone())
                } else {
                    dir_name.clone()
                };

                bundles.push((path, display_name));
            }
        }

        // Sort by directory name descending (newest first)
        bundles.sort_by(|a, b| {
            let a_name = a.0.file_name().unwrap_or_default();
            let b_name = b.0.file_name().unwrap_or_default();
            b_name.cmp(a_name)
        });

        self.available_bundles = bundles;
    }

    /// Set the custom name for the current bundle.
    pub fn set_custom_name(&mut self, name: String) -> Result<(), String> {
        if let Some(ref mut bundle) = self.current_bundle {
            if let Err(e) = bundle.rename(name) {
                return Err(format!("Failed to save bundle name: {}", e));
            }
        }
        Ok(())
    }

    /// Render the bundle info panel.
    ///
    /// Returns an action if the user interacted (copy prompt or switch bundle).
    pub fn render(&mut self, ui: &mut egui::Ui) -> Option<BundleInfoAction> {
        let mut action = None;

        ui.vertical(|ui| {
            ui.set_width(ui.available_width());

            // Panel header with top padding to align with sidebar header
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.heading(egui::RichText::new("Bundle Info").size(16.0));
            });

            ui.add_space(2.0);
            ui.separator();
            ui.add_space(8.0);

            // Bundle selector dropdown
            if !self.available_bundles.is_empty() {
                let current_path = self.current_bundle.as_ref().map(|b| b.path.clone());
                let current_label = current_path
                    .as_ref()
                    .and_then(|p| {
                        self.available_bundles
                            .iter()
                            .find(|(path, _)| path == p)
                            .map(|(_, name)| name.as_str())
                    })
                    .unwrap_or("Select bundle...");

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(icons::FOLDER.to_string()).size(13.0));
                    // Include bundle count in ID so egui discards cached popup size on refresh
                    let combo_id = format!("bundle_selector_{}", self.available_bundles.len());
                    let combo = egui::ComboBox::from_id_salt(combo_id)
                        .width(ui.available_width() - 40.0)
                        .selected_text(current_label);

                    combo.show_ui(ui, |ui| {
                        for (path, name) in &self.available_bundles {
                            let is_selected = current_path.as_ref() == Some(path);
                            if ui
                                .add(egui::Button::selectable(is_selected, name.as_str()))
                                .clicked()
                                && !is_selected
                            {
                                action = Some(BundleInfoAction::SwitchBundle(path.clone()));
                            }
                        }
                    });

                    if ui
                        .button(icons::ARROWS_ROTATE.to_string())
                        .on_hover_text("Refresh bundle list from disk")
                        .clicked()
                    {
                        action = Some(BundleInfoAction::RefreshList);
                    }
                });

                ui.add_space(8.0);
            }

            // Clone data we need before borrowing in closures
            let bundle_data = self.current_bundle.as_ref().map(|b| {
                (
                    b.metadata.name.clone(),
                    b.dir_name().to_string(),
                    b.metadata.config.clone(),
                    b.metadata.created_at,
                    b.metadata.model_info.clone(),
                )
            });

            if let Some((custom_name, dir_name, config, created_at, model_info)) = bundle_data {
                ui.add_space(2.0);
                ui.separator();
                ui.add_space(8.0);

                // Custom Name Input
                // Initialize editing state if not set
                if self.editing_name.is_none() {
                    self.editing_name = Some(custom_name.clone().unwrap_or_default());
                }

                let mut name_text = self.editing_name.clone().unwrap_or_default();
                let mut should_save = false;

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Name:").size(13.0).strong());

                    let response = ui.add(
                        egui::TextEdit::singleline(&mut name_text)
                            .hint_text("Enter custom name...")
                            .desired_width(ui.available_width() - 10.0),
                    );

                    // Track focus state
                    if response.gained_focus() {
                        self.name_has_focus = true;
                    }

                    // Save on Enter or focus loss
                    should_save = (response.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                        || (response.lost_focus() && self.name_has_focus);
                });

                // Save after the closure
                if should_save {
                    self.name_has_focus = false;
                    if !name_text.trim().is_empty() {
                        if let Err(e) = self.set_custom_name(name_text.clone()) {
                            tracing::error!("Failed to save bundle name: {}", e);
                        } else {
                            // Mark save time for showing indicator
                            self.last_save_time = Some(std::time::Instant::now());
                            // Update dropdown label to reflect new name
                            if let Some(ref bundle) = self.current_bundle {
                                if let Some(entry) = self
                                    .available_bundles
                                    .iter_mut()
                                    .find(|(p, _)| *p == bundle.path)
                                {
                                    entry.1 = name_text.clone();
                                }
                            }
                        }
                    }
                }

                self.editing_name = Some(name_text);

                ui.add_space(2.0);

                // Show save hint and status
                ui.horizontal(|ui| {
                    // Check if we should show "Saved!" indicator (show for 2 seconds after save)
                    let show_saved = if let Some(save_time) = self.last_save_time {
                        save_time.elapsed().as_secs_f32() < 2.0
                    } else {
                        false
                    };

                    if show_saved {
                        ui.label(
                            egui::RichText::new(format!("  {} Saved!", icons::CHECK))
                                .size(11.0)
                                .color(egui::Color32::from_rgb(100, 200, 100)),
                        );
                        // Request repaint to clear the indicator after timeout
                        ui.ctx().request_repaint();
                    } else {
                        ui.label(
                            egui::RichText::new("  Press Enter or click away to save")
                                .size(10.0)
                                .weak()
                                .italics(),
                        );
                    }
                });

                ui.add_space(4.0);

                // Show hint about inferred name
                if custom_name.is_none() {
                    ui.label(
                        egui::RichText::new(format!("  (Using directory name: {})", dir_name))
                            .size(10.0)
                            .weak()
                            .italics(),
                    );
                    ui.add_space(4.0);
                }

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(8.0);

                // =================================================================
                // Prompt Section
                // =================================================================
                if let Some(ref config) = config {
                    if let Some(ref prompt) = config.prompt {
                        ui.label(egui::RichText::new("Prompt").strong());
                        ui.add_space(4.0);

                        // Show template if present
                        if let Some(ref template) = config.template {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Template:").size(12.0).weak());
                                ui.label(egui::RichText::new(template).size(12.0));
                            });
                            ui.add_space(2.0);
                        }

                        // Scrollable prompt display
                        egui::ScrollArea::vertical()
                            .max_height(80.0)
                            .show(ui, |ui| {
                                ui.add(
                                    egui::TextEdit::multiline(&mut prompt.as_str())
                                        .interactive(false)
                                        .desired_width(f32::INFINITY)
                                        .frame(true),
                                );
                            });

                        ui.add_space(4.0);

                        // Copy to Input button - right below the prompt
                        if ui
                            .button(format!("{} Copy to Input", icons::COPY))
                            .on_hover_text("Copy this prompt to the input field")
                            .clicked()
                        {
                            action = Some(BundleInfoAction::CopyPrompt(prompt.clone()));
                        }

                        ui.add_space(18.0);
                        ui.separator();
                        ui.add_space(8.0);
                    }
                }

                // =================================================================
                // Metadata Section
                // =================================================================
                ui.label(egui::RichText::new("Metadata").strong());
                ui.add_space(4.0);

                // Creation timestamp
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Created:").size(13.0).weak());
                    ui.label(
                        egui::RichText::new(
                            created_at
                                .with_timezone(&chrono::Local)
                                .format("%Y-%m-%d %H:%M:%S")
                                .to_string(),
                        )
                        .size(13.0),
                    );
                });

                ui.add_space(4.0);

                // Model information (if available)
                if let Some(ref model_info) = model_info {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Vertices:").size(13.0).weak());
                        ui.label(
                            egui::RichText::new(format_number(model_info.vertex_count as u32))
                                .size(13.0),
                        );
                    });
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Triangles:").size(13.0).weak());
                        ui.label(
                            egui::RichText::new(format_number(model_info.triangle_count as u32))
                                .size(13.0),
                        );
                    });
                    ui.add_space(4.0);
                }

                // Model names (if available)
                if let Some(ref config) = config {
                    if let Some(ref image_model) = config.image_model {
                        if !image_model.is_empty() {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Image Model:").size(13.0).weak());
                                ui.label(egui::RichText::new(image_model).size(13.0));
                            });
                            ui.add_space(2.0);
                        }
                    }
                    if !config.model_3d.is_empty() {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("3D Model:").size(13.0).weak());
                            ui.label(egui::RichText::new(&config.model_3d).size(13.0));
                        });
                    }
                }

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(8.0);

                // Export Bundle button
                if let Some(ref bundle) = self.current_bundle {
                    let bundle_path = bundle.path.clone();
                    let default_name = bundle.display_name();
                    if ui
                        .button(format!("{} Export Bundle", icons::DOWNLOAD))
                        .on_hover_text("Save entire bundle as a zip archive")
                        .clicked()
                    {
                        let filename = format!("{}.zip", sanitize_filename(default_name));
                        if let Some(dest) = rfd::FileDialog::new()
                            .set_file_name(&filename)
                            .add_filter("ZIP Archive", &["zip"])
                            .save_file()
                        {
                            action =
                                Some(BundleInfoAction::ExportBundle(bundle_path.clone(), dest));
                        }
                    }
                }
            }

            if self.current_bundle.is_none() && self.available_bundles.is_empty() {
                // No bundle loaded and no bundles available
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    ui.label(
                        egui::RichText::new("No bundles yet")
                            .size(14.0)
                            .weak()
                            .italics(),
                    );
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new("Generate assets to get started")
                            .size(12.0)
                            .weak(),
                    );
                });
            } else if self.current_bundle.is_none() {
                // Bundles available but none selected
                ui.vertical_centered(|ui| {
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new("Select a bundle above")
                            .size(12.0)
                            .weak()
                            .italics(),
                    );
                });
            }
        });

        action
    }
}

/// Sanitize a string for use as a filename.
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Create a zip archive of an entire bundle directory.
/// Re-export for use from app.rs async handler.
pub use asset_tap_core::bundle::export_bundle_zip;

/// Format a number with thousand separators.
fn format_number(n: u32) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.insert(0, ',');
        }
        result.insert(0, c);
    }
    result
}
