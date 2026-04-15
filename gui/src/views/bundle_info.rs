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

/// Duration to show save/add indicators before they fade.
const SAVE_INDICATOR_SECS: f32 = 2.0;

/// Color for success indicators ("Saved!", "Added!").
const SUCCESS_COLOR: egui::Color32 = egui::Color32::from_rgb(100, 200, 100);

/// Color for the favorite star when active.
const FAVORITE_COLOR: egui::Color32 = egui::Color32::from_rgb(255, 200, 50);

/// Render a labeled, read-only, scrollable multi-line text field for prompts.
/// Keeps long prompts from blowing out panel layout.
fn scrollable_prompt_field(
    ui: &mut egui::Ui,
    label: &str,
    text: &str,
    id_salt: &str,
    max_height: f32,
) {
    ui.label(egui::RichText::new(label).size(12.0).weak());
    egui::ScrollArea::vertical()
        .id_salt(id_salt)
        .max_height(max_height)
        .show(ui, |ui| {
            let mut s = text;
            ui.add(
                egui::TextEdit::multiline(&mut s)
                    .interactive(false)
                    .desired_width(f32::INFINITY)
                    .frame(true),
            );
        });
}

/// Render a labeled parameter list as `key: value` lines.
/// Values are JSON-serialized in compact form (booleans, numbers, strings, etc.).
fn render_parameter_list(
    ui: &mut egui::Ui,
    section_label: &str,
    params: &std::collections::HashMap<String, serde_json::Value>,
) {
    ui.label(egui::RichText::new(section_label).size(12.0).weak());
    let mut keys: Vec<&String> = params.keys().collect();
    keys.sort();
    for key in keys {
        let value = &params[key];
        let value_str = match value {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(format!("{}:", key)).size(12.0).weak());
            ui.label(egui::RichText::new(value_str).size(12.0).monospace());
        });
    }
}

/// Show a save/add indicator or a hint label.
fn save_indicator(
    ui: &mut egui::Ui,
    last_time: Option<std::time::Instant>,
    success_msg: &str,
    hint_msg: &str,
) {
    let show_saved = last_time.is_some_and(|t| t.elapsed().as_secs_f32() < SAVE_INDICATOR_SECS);
    if show_saved {
        ui.label(
            egui::RichText::new(format!("  {} {}", icons::CHECK, success_msg))
                .size(11.0)
                .color(SUCCESS_COLOR),
        );
        ui.ctx().request_repaint();
    } else {
        ui.label(
            egui::RichText::new(format!("  {}", hint_msg))
                .size(10.0)
                .weak()
                .italics(),
        );
    }
}

/// Actions the bundle info panel can request from the app.
pub enum BundleInfoAction {
    /// User clicked "Copy to Input" — contains the prompt string.
    CopyPrompt(String),
    /// User selected a different bundle from the dropdown.
    SwitchBundle(PathBuf),
    /// User wants to export the bundle — contains (bundle_dir, destination_path).
    ExportBundle(PathBuf, PathBuf),
    /// User wants to import a bundle from a zip file.
    ImportBundle(PathBuf),
    /// User wants to delete the current bundle.
    DeleteBundle(PathBuf),
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
    /// Available bundles: (directory path, display name, is_favorite).
    available_bundles: Vec<(PathBuf, String, bool)>,
    /// Tag currently being typed.
    tag_input: String,
    /// Notes text being edited.
    editing_notes: Option<String>,
    /// Timestamp when a tag was last added (for showing "Added!" indicator).
    last_tag_save_time: Option<std::time::Instant>,
    /// Timestamp when notes were last saved (for showing "Saved!" indicator).
    last_notes_save_time: Option<std::time::Instant>,
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
            tag_input: String::new(),
            editing_notes: None,
            last_tag_save_time: None,
            last_notes_save_time: None,
        }
    }

    /// Load a bundle into the panel.
    pub fn load_bundle(&mut self, bundle_path: PathBuf) -> Result<(), String> {
        match asset_tap_core::bundle::load_bundle(&bundle_path) {
            Ok(bundle) => {
                self.editing_notes = bundle.metadata.notes.clone();
                self.current_bundle = Some(bundle);
                self.editing_name = None;
                self.tag_input.clear();
                self.last_save_time = None;
                self.last_tag_save_time = None;
                self.last_notes_save_time = None;
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

                // Skip directories that don't look like bundles — empty dirs
                // (e.g., partial/interrupted generations) were causing selection
                // regressions in the browser.
                if !asset_tap_core::bundle::looks_like_bundle(&path) {
                    continue;
                }

                // Try to read custom name and favorite status from bundle.json
                let bundle_json = path.join(bundle_files::METADATA);
                let (display_name, is_favorite) = if bundle_json.exists() {
                    let json = std::fs::read_to_string(&bundle_json)
                        .ok()
                        .and_then(|contents| {
                            serde_json::from_str::<serde_json::Value>(&contents).ok()
                        });
                    let name = json
                        .as_ref()
                        .and_then(|j| j.get("name")?.as_str().map(|s| s.to_string()))
                        .filter(|name| !name.is_empty())
                        .unwrap_or_else(|| dir_name.clone());
                    let fav = json
                        .as_ref()
                        .and_then(|j| j.get("favorite")?.as_bool())
                        .unwrap_or(false);
                    (name, fav)
                } else {
                    (dir_name.clone(), false)
                };

                bundles.push((path, display_name, is_favorite));
            }
        }

        // Sort: favorites first, then by directory name descending (newest first)
        bundles.sort_by(|a, b| {
            b.2.cmp(&a.2).then_with(|| {
                let a_name = a.0.file_name().unwrap_or_default();
                let b_name = b.0.file_name().unwrap_or_default();
                b_name.cmp(a_name)
            })
        });

        self.available_bundles = bundles;
    }

    /// Set the custom name for the current bundle.
    pub fn set_custom_name(&mut self, name: String) -> Result<(), String> {
        if let Some(ref mut bundle) = self.current_bundle
            && let Err(e) = bundle.rename(name)
        {
            return Err(format!("Failed to save bundle name: {}", e));
        }
        Ok(())
    }

    /// Render the bundle info panel.
    ///
    /// Returns an action if the user interacted (copy prompt or switch bundle).
    pub fn render(&mut self, ui: &mut egui::Ui) -> Option<BundleInfoAction> {
        let mut action = None;

        // Panel header (aligned with sidebar and preview headers)
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.heading("Bundle Info");
        });
        ui.separator();
        ui.add_space(4.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            // Bundle selector dropdown
            if !self.available_bundles.is_empty() {
                let current_path = self.current_bundle.as_ref().map(|b| b.path.clone());
                let current_label = current_path
                    .as_ref()
                    .and_then(|p| {
                        self.available_bundles
                            .iter()
                            .find(|(path, _, _)| path == p)
                            .map(|(_, name, fav)| {
                                if *fav {
                                    format!("{} {}", icons::STAR, name)
                                } else {
                                    name.clone()
                                }
                            })
                    })
                    .unwrap_or_else(|| "Select bundle...".to_string());

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(icons::FOLDER.to_string()).size(13.0));
                    // Include bundle count in ID so egui discards cached popup size on refresh
                    let combo_id = format!("bundle_selector_{}", self.available_bundles.len());
                    let combo = egui::ComboBox::from_id_salt(combo_id)
                        .width(ui.available_width() - 70.0)
                        .selected_text(current_label);

                    combo.show_ui(ui, |ui| {
                        for (path, name, fav) in &self.available_bundles {
                            let is_selected = current_path.as_ref() == Some(path);
                            let label = if *fav {
                                format!("{} {}", icons::STAR, name)
                            } else {
                                name.clone()
                            };
                            if ui
                                .add(egui::Button::selectable(is_selected, label))
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

                    if ui
                        .button(icons::FOLDER_OPEN.to_string())
                        .on_hover_text("Import a bundle from a zip archive")
                        .clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .add_filter("Bundle Archive", &["zip"])
                            .pick_file()
                    {
                        action = Some(BundleInfoAction::ImportBundle(path));
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
                    b.metadata.generator.clone(),
                    b.metadata.tags.clone(),
                    b.metadata.favorite,
                )
            });

            if let Some((
                custom_name,
                dir_name,
                config,
                created_at,
                model_info,
                generator,
                tags,
                favorite,
            )) = bundle_data
            {
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
                            .char_limit(asset_tap_core::constants::validation::MAX_NAME_LENGTH)
                            .desired_width(ui.available_width() - 45.0),
                    );

                    // Favorite star next to name
                    if ui
                        .button(
                            egui::RichText::new(icons::STAR.to_string())
                                .size(14.0)
                                .color(if favorite {
                                    FAVORITE_COLOR
                                } else {
                                    ui.visuals().weak_text_color()
                                }),
                        )
                        .on_hover_text(if favorite {
                            "Remove from favorites"
                        } else {
                            "Mark as favorite"
                        })
                        .clicked()
                        && let Some(ref mut bundle) = self.current_bundle
                    {
                        bundle.metadata.toggle_favorite();
                        let _ = bundle.save();
                        if let Some(entry) = self
                            .available_bundles
                            .iter_mut()
                            .find(|(p, _, _)| *p == bundle.path)
                        {
                            entry.2 = bundle.metadata.favorite;
                        }
                    }

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
                            if let Some(ref bundle) = self.current_bundle
                                && let Some(entry) = self
                                    .available_bundles
                                    .iter_mut()
                                    .find(|(p, _, _)| *p == bundle.path)
                            {
                                entry.1 = name_text.clone();
                            }
                        }
                    }
                }

                self.editing_name = Some(name_text);

                ui.add_space(2.0);

                // Show save hint and status
                save_indicator(
                    ui,
                    self.last_save_time,
                    "Saved!",
                    "Press Enter or click away to save",
                );

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
                if let Some(ref config) = config
                    && let Some(ref prompt) = config.prompt
                {
                    ui.label(egui::RichText::new("Prompt").strong());
                    ui.add_space(4.0);

                    // Show template and original input if present
                    if let Some(ref template) = config.template {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Template:").size(12.0).weak());
                            ui.label(egui::RichText::new(template).size(12.0));
                        });
                        ui.add_space(2.0);

                        if let Some(ref user_prompt) = config.user_prompt {
                            scrollable_prompt_field(
                                ui,
                                "Original input:",
                                user_prompt,
                                "bundle_info_user_prompt",
                                60.0,
                            );
                            ui.add_space(2.0);
                        }
                    }

                    let expanded_label = if config.template.is_some() {
                        "Expanded prompt:"
                    } else {
                        "Prompt:"
                    };
                    scrollable_prompt_field(ui, expanded_label, prompt, "bundle_info_prompt", 80.0);

                    ui.add_space(4.0);

                    // Copy to Input — prefer original user input when a template was used
                    let copy_text = config.user_prompt.as_deref().unwrap_or(prompt).to_string();
                    if ui
                        .button(format!("{} Copy to Input", icons::COPY))
                        .on_hover_text("Copy this prompt to the input field")
                        .clicked()
                    {
                        action = Some(BundleInfoAction::CopyPrompt(copy_text));
                    }

                    ui.add_space(18.0);
                    ui.separator();
                    ui.add_space(8.0);
                }

                // =================================================================
                // Parameters Section (only shown when overrides were applied)
                // =================================================================
                if let Some(ref config) = config {
                    let has_image_params = !config.image_model_params.is_empty();
                    let has_model_3d_params = !config.model_3d_params.is_empty();
                    if has_image_params || has_model_3d_params {
                        ui.label(egui::RichText::new("Parameters").strong());
                        ui.add_space(4.0);
                        if has_image_params {
                            render_parameter_list(ui, "Image model:", &config.image_model_params);
                        }
                        if has_model_3d_params {
                            if has_image_params {
                                ui.add_space(4.0);
                            }
                            render_parameter_list(ui, "3D model:", &config.model_3d_params);
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

                // Generator
                if let Some(ref generator_id) = generator {
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Generator:").size(13.0).weak());
                        ui.label(egui::RichText::new(generator_id).size(13.0));
                    });
                }

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
                    if let Some(ref image_model) = config.image_model
                        && !image_model.is_empty()
                    {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Image Model:").size(13.0).weak());
                            ui.label(egui::RichText::new(image_model).size(13.0));
                        });
                        ui.add_space(2.0);
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

                // =================================================================
                // Tags & Notes Section
                // =================================================================

                // Tags
                ui.label(egui::RichText::new("Tags").size(13.0).strong());
                ui.add_space(4.0);

                // Display existing tags as removable chips
                if !tags.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        let mut tag_to_remove = None;
                        for tag in &tags {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                ui.label(egui::RichText::new(tag).size(12.0));
                                if ui
                                    .small_button(icons::XMARK.to_string())
                                    .on_hover_text("Remove tag")
                                    .clicked()
                                {
                                    tag_to_remove = Some(tag.clone());
                                }
                            });
                        }
                        if let Some(tag) = tag_to_remove
                            && let Some(ref mut bundle) = self.current_bundle
                        {
                            bundle.metadata.remove_tag(&tag);
                            let _ = bundle.save();
                        }
                    });
                    ui.add_space(4.0);
                }

                // Add tag input
                let max_tags = asset_tap_core::constants::validation::MAX_TAGS;
                if tags.len() < max_tags {
                    ui.horizontal(|ui| {
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut self.tag_input)
                                .hint_text("Add tag...")
                                .char_limit(asset_tap_core::constants::validation::MAX_TAG_LENGTH)
                                .desired_width(ui.available_width() - 16.0),
                        );

                        let submit = response.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter))
                            && !self.tag_input.trim().is_empty();

                        if submit {
                            if let Some(ref mut bundle) = self.current_bundle {
                                bundle.metadata.add_tag(self.tag_input.trim());
                                let _ = bundle.save();
                                self.last_tag_save_time = Some(std::time::Instant::now());
                            }
                            self.tag_input.clear();
                        }
                    });

                    // Save hint / indicator
                    save_indicator(ui, self.last_tag_save_time, "Added!", "Press Enter to add");
                } else {
                    ui.label(
                        egui::RichText::new(format!("Maximum {} tags reached", max_tags))
                            .size(11.0)
                            .weak()
                            .italics(),
                    );
                }

                ui.add_space(8.0);

                // Notes
                ui.label(egui::RichText::new("Notes").size(13.0).strong());
                ui.add_space(4.0);

                let mut notes_text = self.editing_notes.clone().unwrap_or_default();
                let notes_response = ui.add(
                    egui::TextEdit::multiline(&mut notes_text)
                        .hint_text("Add notes...")
                        .char_limit(asset_tap_core::constants::validation::MAX_NOTES_LENGTH)
                        .desired_width(ui.available_width() - 16.0)
                        .desired_rows(3),
                );

                // Save notes when focus is lost and content changed
                if notes_response.lost_focus() {
                    let new_notes = if notes_text.trim().is_empty() {
                        None
                    } else {
                        Some(notes_text.clone())
                    };
                    if let Some(ref mut bundle) = self.current_bundle
                        && bundle.metadata.notes != new_notes
                    {
                        bundle.metadata.notes = new_notes;
                        let _ = bundle.save();
                        self.last_notes_save_time = Some(std::time::Instant::now());
                    }
                }
                self.editing_notes = Some(notes_text);

                // Save hint / indicator
                save_indicator(
                    ui,
                    self.last_notes_save_time,
                    "Saved!",
                    "Click away to save",
                );

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(8.0);

                // Export + Delete buttons on the same line
                if let Some(ref bundle) = self.current_bundle {
                    let bundle_path = bundle.path.clone();
                    let has_name = bundle.metadata.name.is_some();
                    let display_name = bundle.display_name().to_string();

                    ui.horizontal(|ui| {
                        let export_button = ui.add_enabled(
                            has_name,
                            egui::Button::new(format!("{} Export", icons::DOWNLOAD)),
                        );

                        if has_name {
                            if export_button
                                .on_hover_text("Save entire bundle as a zip archive")
                                .clicked()
                            {
                                let filename = format!("{}.zip", sanitize_filename(&display_name));
                                if let Some(dest) = rfd::FileDialog::new()
                                    .set_file_name(&filename)
                                    .add_filter("ZIP Archive", &["zip"])
                                    .save_file()
                                {
                                    action = Some(BundleInfoAction::ExportBundle(
                                        bundle_path.clone(),
                                        dest,
                                    ));
                                }
                            }
                        } else {
                            export_button
                                .on_disabled_hover_text("Name this bundle before exporting");
                        }

                        if ui
                            .button(
                                egui::RichText::new(format!("{} Delete", icons::TRASH))
                                    .color(egui::Color32::from_rgb(255, 100, 100)),
                            )
                            .on_hover_text("Permanently delete this bundle")
                            .clicked()
                        {
                            action = Some(BundleInfoAction::DeleteBundle(bundle_path));
                        }
                    });
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
