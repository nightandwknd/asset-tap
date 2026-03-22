//! Settings modal for configuring paths and API keys.

use crate::icons;
use crate::style::RichTextExt;
use asset_tap_core::convert::find_blender;
use asset_tap_core::settings::{is_dev_mode, settings_file_path, Settings};
use eframe::egui;
use std::collections::HashMap;
use std::path::PathBuf;

/// State for the settings modal.
pub struct SettingsModal {
    /// Whether the modal is open.
    pub is_open: bool,

    /// Working copy of settings (edited in UI).
    pub draft: SettingsDraft,

    /// Original draft state when modal was opened (for change detection).
    original: SettingsDraft,

    /// Whether to show API keys (vs masked) for each provider.
    show_api_keys: HashMap<String, bool>,

    /// Status message to display with timestamp.
    pub status_message: Option<(String, bool, std::time::Instant)>, // (message, is_error, timestamp)
}

/// Draft settings being edited in the UI.
#[derive(Clone, Default)]
pub struct SettingsDraft {
    pub output_dir: String,
    pub blender_path: String,
    pub blender_auto_detected: Option<String>,
    /// Provider API keys (provider_id -> key).
    pub provider_api_keys: HashMap<String, String>,
    /// Providers from environment (provider_id -> bool).
    pub provider_keys_from_env: HashMap<String, bool>,
    // UI preferences
    pub require_image_approval: bool,
}

impl SettingsModal {
    pub fn new() -> Self {
        // Create an empty draft - will be properly initialized in open()
        let draft = SettingsDraft::default();
        Self {
            is_open: false,
            original: draft.clone(),
            draft,
            show_api_keys: HashMap::new(),
            status_message: None,
        }
    }

    /// Open the settings modal.
    pub fn open(
        &mut self,
        settings: &Settings,
        registry: &asset_tap_core::providers::ProviderRegistry,
    ) {
        self.is_open = true;
        self.draft = SettingsDraft::from_settings(settings, registry);
        self.original = self.draft.clone();
        self.show_api_keys.clear(); // Reset to hidden when opening
        self.status_message = None;
    }

    /// Check if there are unsaved changes.
    pub fn has_changes(&self) -> bool {
        self.draft.differs_from(&self.original)
    }

    /// Close the settings modal.
    pub fn close(&mut self) {
        self.is_open = false;
    }

    /// Apply draft changes to the actual settings.
    pub fn apply(
        &mut self,
        settings: &mut Settings,
        registry: &asset_tap_core::providers::ProviderRegistry,
    ) -> Result<(), String> {
        // Validate output directory
        let output_dir = PathBuf::from(&self.draft.output_dir);
        if self.draft.output_dir.is_empty() {
            return Err("Output directory cannot be empty".to_string());
        }

        // Apply changes
        settings.output_dir = output_dir;

        // Blender path (empty string means auto-detect)
        settings.blender_path = if self.draft.blender_path.is_empty() {
            None
        } else {
            Some(self.draft.blender_path.clone())
        };

        // Provider API keys
        for (provider_id, key) in &self.draft.provider_api_keys {
            settings.set_provider_api_key(provider_id.clone(), key.clone());
        }

        // Sync settings to environment variables (for GUI app)
        settings.sync_to_env(registry);

        // UI Preferences
        settings.require_image_approval = self.draft.require_image_approval;

        // Save to disk
        settings
            .save()
            .map_err(|e| format!("Failed to save settings: {}", e))?;

        // Ensure output directory exists
        settings
            .ensure_output_dir()
            .map_err(|e| format!("Failed to create output directory: {}", e))?;

        // Update original to match current draft (no more changes)
        self.original = self.draft.clone();
        self.status_message = Some((
            "Settings saved successfully".to_string(),
            false,
            std::time::Instant::now(),
        ));

        Ok(())
    }

    /// Render the settings modal.
    /// Returns true if settings were saved.
    pub fn render(
        &mut self,
        ctx: &egui::Context,
        settings: &mut Settings,
        registry: &asset_tap_core::providers::ProviderRegistry,
    ) -> bool {
        if !self.is_open {
            return false;
        }

        // Auto-clear status messages after 3 seconds
        if let Some((_, _, timestamp)) = &self.status_message {
            if timestamp.elapsed().as_secs() >= 3 {
                self.status_message = None;
            }
        }

        let mut saved = false;
        let mut should_close = false;
        let mut clicked_cancel = false;

        // Draw backdrop (click outside to close and discard changes)
        egui::Area::new(egui::Id::new("settings_backdrop"))
            .fixed_pos(egui::pos2(0.0, 0.0))
            .order(egui::Order::Background)
            .show(ctx, |ui| {
                let screen_rect = ctx.screen_rect();

                // Allow clicking outside only if output directory is set (required field)
                let can_close = !self.draft.output_dir.is_empty();
                if can_close {
                    let response = ui.allocate_response(screen_rect.size(), egui::Sense::click());
                    if response.clicked() {
                        should_close = true;
                    }
                } else {
                    ui.allocate_response(screen_rect.size(), egui::Sense::hover());
                }

                ui.painter()
                    .rect_filled(screen_rect, 0.0, egui::Color32::from_black_alpha(180));
            });

        // Modal window
        egui::Window::new(format!("{} Settings", icons::GEAR))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_width(500.0);
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 12.0);

                // Wrap content in a scroll area to handle overflow
                egui::ScrollArea::vertical()
                    .max_height(ctx.screen_rect().height() - 150.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        // Dev mode indicator
                        if is_dev_mode() {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} Development Mode",
                                        icons::CODE
                                    ))
                                    .color(egui::Color32::from_rgb(100, 200, 255))
                                    .small(),
                                );
                            });
                            ui.separator();
                        }

                        // =============================================================
                        // Paths Section
                        // =============================================================
                        ui.heading("Paths");

                        // Output Directory
                        ui.horizontal(|ui| {
                            ui.label("Output Directory:");
                        });
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.draft.output_dir)
                                    .desired_width(380.0)
                                    .hint_text("Where generated assets are saved"),
                            );
                            if ui.button(icons::FOLDER_OPEN).on_hover_text("Browse for output directory").clicked() {
                                if let Some(path) = rfd::FileDialog::new()
                                    .set_directory(&self.draft.output_dir)
                                    .pick_folder()
                                {
                                    self.draft.output_dir = path.display().to_string();
                                }
                            }
                        });

                        ui.add_space(4.0);

                        // Blender Path
                        ui.horizontal(|ui| {
                            ui.label("Blender Path:");
                            if self.draft.blender_auto_detected.is_some() {
                                ui.label(
                                    egui::RichText::new("(auto-detected)")
                                        .small()
                                        .color(egui::Color32::GRAY),
                                );
                            }
                        });
                        ui.horizontal(|ui| {
                            let hint = self
                                .draft
                                .blender_auto_detected
                                .as_deref()
                                .unwrap_or("Not found - enter path manually");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.draft.blender_path)
                                    .desired_width(380.0)
                                    .hint_text(hint),
                            );
                            if ui.button(icons::FOLDER_OPEN).on_hover_text("Browse for Blender executable").clicked() {
                                if let Some(path) = rfd::FileDialog::new().pick_file() {
                                    self.draft.blender_path = path.display().to_string();
                                }
                            }
                        });

                        if let Some(ref detected) = self.draft.blender_auto_detected {
                            if self.draft.blender_path.is_empty() {
                                ui.label(
                                    egui::RichText::new(format!("Using: {}", detected))
                                        .small()
                                        .color(egui::Color32::from_rgb(100, 200, 100)),
                                );
                            }
                        }

                        ui.add_space(8.0);
                        ui.separator();

                        // =============================================================
                        // Provider Loading Errors (if any)
                        // =============================================================
                        if registry.has_load_errors() {
                            ui.heading("Provider Errors");
                            ui.label(
                                egui::RichText::new(
                                    "The following provider configurations failed to load:",
                                )
                                .color(egui::Color32::from_rgb(255, 180, 100)),
                            );
                            ui.add_space(4.0);

                            for error in registry.get_load_errors() {
                                ui.group(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(crate::icons::WARNING)
                                                .color(egui::Color32::from_rgb(255, 180, 100)),
                                        );
                                        ui.label(
                                            egui::RichText::new(&error.path).strong().monospace(),
                                        );
                                    });
                                    ui.label(
                                        egui::RichText::new(&error.error)
                                            .small()
                                            .color(egui::Color32::from_rgb(200, 200, 200)),
                                    );
                                });
                                ui.add_space(4.0);
                            }

                            ui.add_space(16.0);
                            ui.separator();
                            ui.add_space(16.0);
                        }

                        // =============================================================
                        // Template Loading Errors (if any)
                        // =============================================================
                        // Clone errors out of the registry so we don't hold the lock during rendering
                        let template_errors: Vec<_> = {
                            let template_registry = asset_tap_core::templates::REGISTRY.read().unwrap();
                            template_registry.load_errors.clone()
                        };
                        if !template_errors.is_empty() {
                            ui.heading("Template Errors");
                            ui.label(
                                egui::RichText::new(
                                    "The following template files failed to load:",
                                )
                                .color(egui::Color32::from_rgb(255, 180, 100)),
                            );
                            ui.add_space(4.0);

                            for error in &template_errors {
                                ui.group(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(crate::icons::WARNING)
                                                .color(egui::Color32::from_rgb(255, 180, 100)),
                                        );
                                        ui.label(
                                            egui::RichText::new(&error.path).strong().monospace(),
                                        );
                                        ui.label(
                                            egui::RichText::new(format!("[{:?}]", error.kind))
                                                .small()
                                                .color(egui::Color32::from_rgb(150, 150, 150)),
                                        );
                                    });
                                    ui.label(
                                        egui::RichText::new(&error.error)
                                            .small()
                                            .color(egui::Color32::from_rgb(200, 200, 200)),
                                    );
                                });
                                ui.add_space(4.0);
                            }

                            ui.add_space(16.0);
                            ui.separator();
                            ui.add_space(16.0);
                        }

                        // =============================================================
                        // API Keys Section
                        // =============================================================
                        ui.heading("API Keys");

                        let all_providers = registry.list_all();

                        // Render key input for each provider
                        for provider in &all_providers {
                            let provider_id = provider.id();
                            let provider_name = provider.name();

                            ui.horizontal(|ui| {
                                ui.label(format!("{} API Key:", provider_name));
                                if *self
                                    .draft
                                    .provider_keys_from_env
                                    .get(provider_id)
                                    .unwrap_or(&false)
                                {
                                    ui.label(
                                        egui::RichText::new("(from .env)")
                                            .small()
                                            .color(egui::Color32::from_rgb(100, 200, 255)),
                                    );
                                }
                            });

                            ui.horizontal(|ui| {
                                // Get or create empty key string
                                let key = self
                                    .draft
                                    .provider_api_keys
                                    .entry(provider_id.to_string())
                                    .or_default();

                                // Get show/hide state for this provider
                                let show_key = *self
                                    .show_api_keys
                                    .entry(provider_id.to_string())
                                    .or_insert(false);

                                // Generate hint text from env var name or use generic
                                let hint_text = if let Some(env_var) = provider.metadata().required_env_vars.first() {
                                    format!("{} value", env_var)
                                } else {
                                    "API key".to_string()
                                };

                                ui.add(
                                    egui::TextEdit::singleline(key)
                                        .desired_width(360.0)
                                        .password(!show_key)
                                        .hint_text(hint_text),
                                );


                                // Eye icon to show/hide
                                let (icon, tooltip) = if show_key {
                                    (icons::EYE_SLASH, "Hide")
                                } else {
                                    (icons::EYE, "Show")
                                };
                                if ui.small_button(icon).on_hover_text(tooltip).clicked() {
                                    let current =
                                        *self.show_api_keys.get(provider_id).unwrap_or(&false);
                                    self.show_api_keys.insert(provider_id.to_string(), !current);
                                }
                            });

                            ui.horizontal(|ui| {
                                // Get API key URL from provider metadata
                                let provider = registry.get(provider_id);
                                let (key_url, key_url_text) = if let Some(provider) = provider {
                                    let metadata = provider.metadata();
                                    if let Some(url) = &metadata.api_key_url {
                                        (url.clone(), format!("Get your API key at {}", url))
                                    } else {
                                        (
                                            String::new(),
                                            "Check provider documentation for API key".to_string(),
                                        )
                                    }
                                } else {
                                    (String::new(), "Configure in provider settings".to_string())
                                };

                                ui.label(egui::RichText::new(&key_url_text).small().secondary());

                                if !key_url.is_empty()
                                    && ui
                                        .small_button(icons::ARROW_SQUARE_OUT)
                                        .on_hover_text("Open in browser")
                                        .clicked()
                                {
                                    crate::app::open_with_system(&key_url, None);
                                }
                            });

                            ui.add_space(8.0);
                        }

                        ui.separator();

                        // =============================================================
                        // UI Preferences
                        // =============================================================
                        ui.heading("UI Preferences");

                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.draft.require_image_approval, "")
                                .on_hover_text("When enabled, you'll be prompted to review the generated image before proceeding to 3D generation");
                            ui.label("Require image approval before 3D generation");
                        });

                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new("📝 Review generated images before creating 3D models")
                                .small()
                                .color(egui::Color32::GRAY),
                        );

                        ui.add_space(8.0);
                        ui.separator();

                        // =============================================================
                        // Settings File Link
                        // =============================================================
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("{} Settings file:", icons::FILE))
                                    .small()
                                    .secondary(),
                            );
                            let path = settings_file_path();
                            let path_str = path.display().to_string();
                            if ui
                                .link(egui::RichText::new(&path_str).small())
                                .on_hover_text("Open settings file location")
                                .clicked()
                            {
                                if let Some(parent) = path.parent() {
                                    crate::app::open_with_system(parent, None);
                                }
                            }
                        });

                        // =============================================================
                        // Application Logs Link
                        // =============================================================
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} Application logs:",
                                    icons::TERMINAL
                                ))
                                .small()
                                .secondary(),
                            );
                            let logs_path =
                                asset_tap_core::error_log::logs_dir_path();
                            let logs_str = logs_path.display().to_string();
                            if ui
                                .link(egui::RichText::new(&logs_str).small())
                                .on_hover_text("Open logs directory")
                                .clicked()
                            {
                                std::fs::create_dir_all(&logs_path).ok();
                                crate::app::open_with_system(&logs_path, None);
                            }
                        });

                        ui.add_space(8.0);
                    }); // End ScrollArea

                // =============================================================
                // Status Message (outside scroll area)
                // =============================================================
                if let Some((msg, is_error, _)) = &self.status_message {
                    let color = if *is_error {
                        egui::Color32::from_rgb(255, 100, 100)
                    } else {
                        egui::Color32::from_rgb(100, 200, 100)
                    };
                    ui.label(egui::RichText::new(msg).color(color));
                }

                // =============================================================
                // Buttons (outside scroll area)
                // =============================================================
                ui.add_space(4.0);
                let has_changes = self.has_changes();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            has_changes,
                            egui::Button::new(format!("{} Save", icons::FLOPPY_DISK)),
                        )
                        .clicked()
                    {
                        match self.apply(settings, registry) {
                            Ok(()) => {
                                saved = true;
                                should_close = true;
                            }
                            Err(e) => self.status_message = Some((e, true, std::time::Instant::now())),
                        }
                    }

                    // Only allow cancel if output directory is set (required field)
                    let can_cancel = !self.draft.output_dir.is_empty();
                    if ui
                        .add_enabled(can_cancel, egui::Button::new("Cancel"))
                        .on_disabled_hover_text("Output directory is required")
                        .clicked()
                    {
                        clicked_cancel = true;
                        should_close = true;
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button(format!(
                                "{} Reset to Defaults",
                                icons::ARROW_COUNTER_CLOCKWISE
                            ))
                            .clicked()
                        {
                            let mut default_draft =
                                SettingsDraft::from_settings(&Settings::default(), registry);
                            // Re-detect Blender
                            default_draft.blender_auto_detected = find_blender();
                            // Preserve the from_env flags from current draft
                            default_draft.provider_keys_from_env =
                                self.draft.provider_keys_from_env.clone();
                            self.draft = default_draft;
                        }
                    });
                });
            });

        if should_close {
            // If Cancel button was clicked OR user clicked outside, revert draft to original state
            if clicked_cancel || !saved {
                self.draft = self.original.clone();
                self.status_message = None;
            }
            self.close();
        }

        saved
    }
}

impl Default for SettingsModal {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsDraft {
    /// Create a draft from current settings.
    pub fn from_settings(
        settings: &Settings,
        registry: &asset_tap_core::providers::ProviderRegistry,
    ) -> Self {
        let auto_detected = find_blender();

        // Get all provider keys using core helper (respects .env priority, fully dynamic)
        let all_keys = settings.get_all_provider_keys(registry);
        let mut provider_api_keys = HashMap::new();
        let mut provider_keys_from_env = HashMap::new();

        for (provider_id, (key, is_from_env)) in all_keys {
            provider_api_keys.insert(provider_id.clone(), key);
            if is_from_env {
                provider_keys_from_env.insert(provider_id, true);
            }
        }

        Self {
            output_dir: settings.output_dir.display().to_string(),
            blender_path: settings.blender_path.clone().unwrap_or_default(),
            blender_auto_detected: auto_detected,
            provider_api_keys,
            provider_keys_from_env,
            require_image_approval: settings.require_image_approval,
        }
    }

    /// Check if this draft differs from another (ignoring auto_detected and from_env flags).
    pub fn differs_from(&self, other: &SettingsDraft) -> bool {
        self.output_dir != other.output_dir
            || self.blender_path != other.blender_path
            || self.provider_api_keys != other.provider_api_keys
            || self.require_image_approval != other.require_image_approval
    }
}
