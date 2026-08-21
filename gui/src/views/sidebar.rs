//! Configuration sidebar panel.

use crate::app::{App, Toast};
use crate::icons;
use crate::style::RichTextExt;
use crate::views::walkthrough::WalkthroughStep;
use asset_tap_core::constants::files::APP_DISPLAY_NAME;
use asset_tap_core::providers::ProviderCapability;
use eframe::egui;
use std::path::PathBuf;

/// Render the configuration sidebar.
pub fn render(app: &mut App, ui: &mut egui::Ui) {
    // Register sidebar panel rect for walkthrough
    app.walkthrough
        .register_rect(WalkthroughStep::SidebarPanel, ui.max_rect());

    // Header (outside ScrollArea so it aligns with other panel headers)
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.heading(APP_DISPLAY_NAME);

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(icons::GEAR).on_hover_text("Settings").clicked() {
                app.settings_modal
                    .open(&app.settings, &app.provider_registry);
            }
        });
    });
    ui.separator();

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(4.0);

        // =================================================================
        // Prompt Section
        // =================================================================
        ui.label(egui::RichText::new("Prompt").strong());
        ui.add_space(4.0);

        // Template selector with view button
        let has_existing_image = app.existing_image.is_some();

        let template_row = ui.add_enabled_ui(!has_existing_image, |ui| {
            ui.horizontal(|ui| {
                ui.label("Template:");
                egui::ComboBox::from_id_salt("template_selector")
                    .selected_text(app.template.as_deref().unwrap_or("None"))
                    .show_ui(ui, |ui| {
                        if ui
                            .add(egui::Button::selectable(app.template.is_none(), "None"))
                            .clicked()
                        {
                            app.template = None;
                        }

                        ui.separator();

                        for template in &app.available_templates {
                            if ui
                                .add(egui::Button::selectable(
                                    app.template.as_deref() == Some(template.as_str()),
                                    template.as_str(),
                                ))
                                .clicked()
                            {
                                app.template = Some(template.to_string());
                            }
                        }

                        ui.separator();

                        if ui
                            .add(egui::Button::selectable(false, "+ Create Custom Template"))
                            .clicked()
                        {
                            open_template_for_creation(app);
                        }
                    });

                // View button on the right (only when a template is selected)
                if let Some(template_name) = &app.template.clone() {
                    if ui
                        .small_button(icons::CODE)
                        .on_hover_text("View template syntax")
                        .clicked()
                    {
                        open_template_for_viewing(app, template_name);
                    }

                    // Delete button for custom templates only
                    if let Some(template) =
                        asset_tap_core::templates::get_template_definition(template_name)
                        && !template.is_builtin
                        && ui
                            .small_button(icons::TRASH)
                            .on_hover_text("Delete template")
                            .clicked()
                    {
                        delete_custom_template(app, template_name);
                    }
                }
            });
        });
        app.walkthrough.register_rect(
            WalkthroughStep::TemplateSelector,
            template_row.response.rect,
        );

        ui.add_space(4.0);

        // Prompt input
        let prompt_hint = if app.template.is_some() {
            "Describe your character..."
        } else {
            "Describe what you want to create..."
        };

        let prompt_response = ui.add_enabled_ui(!has_existing_image, |ui| {
            ui.add_sized(
                [ui.available_width(), 80.0],
                egui::TextEdit::multiline(&mut app.prompt)
                    .hint_text(if has_existing_image {
                        "Prompt disabled — using existing image for 3D generation"
                    } else {
                        prompt_hint
                    })
                    .desired_width(f32::INFINITY),
            )
        });
        app.walkthrough
            .register_rect(WalkthroughStep::PromptInput, prompt_response.response.rect);

        // Character counter (shown when prompt is getting long)
        let max_len = asset_tap_core::constants::validation::MAX_PROMPT_LENGTH;
        let effective_len = app.effective_prompt_len();
        if effective_len > max_len * 8 / 10 {
            let color = if effective_len > max_len {
                egui::Color32::RED
            } else {
                egui::Color32::YELLOW
            };
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                ui.colored_label(color, format!("{}/{}", effective_len, max_len));
            });
        }

        // Prompt history dropdown (disabled when using existing image)
        ui.add_space(4.0);
        ui.add_enabled_ui(!has_existing_image, |ui| {
            ui.horizontal(|ui| {
                let history_text = if app.app_state.prompt_history.is_empty() {
                    "No history yet".to_string()
                } else {
                    format!("{} recent", app.app_state.prompt_history.len())
                };

                ui.label(egui::RichText::new("History:").size(12.0).weak());
                egui::ComboBox::from_id_salt("prompt_history")
                    .selected_text(history_text)
                    .width(ui.available_width() - 10.0)
                    .show_ui(ui, |ui| {
                        if app.app_state.prompt_history.is_empty() {
                            ui.label(
                                egui::RichText::new("Generate something to build history")
                                    .weak()
                                    .italics(),
                            );
                        } else {
                            for entry in app.app_state.prompt_history.iter() {
                                // Truncate long prompts for display
                                let display_prompt = if entry.prompt.chars().count() > 50 {
                                    let truncated: String = entry.prompt.chars().take(47).collect();
                                    format!("{truncated}...")
                                } else {
                                    entry.prompt.clone()
                                };

                                // Add template indicator if present
                                let display = if let Some(ref template) = entry.template {
                                    format!("{} [{}]", display_prompt, template)
                                } else {
                                    display_prompt
                                };

                                if ui.add(egui::Button::selectable(false, display)).clicked() {
                                    app.prompt = entry.prompt.clone();
                                    app.template = entry.template.clone();
                                }
                            }

                            // Clear history option
                            if !app.app_state.prompt_history.is_empty() {
                                ui.separator();
                                if ui
                                    .add(egui::Button::selectable(
                                        false,
                                        format!("{} Clear history", icons::X),
                                    ))
                                    .clicked()
                                {
                                    app.show_clear_history_confirmation = true;
                                }
                            }
                        }
                    });
            });
        });

        // Warning if both prompt and existing image are set
        if !app.prompt.is_empty() && app.existing_image.is_some() {
            ui.add_space(4.0);
            egui::Frame::new()
                .fill(egui::Color32::from_rgb(80, 60, 40))
                .corner_radius(4)
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.colored_label(egui::Color32::from_rgb(255, 200, 100), icons::WARNING);
                        ui.label(
                            egui::RichText::new("Existing image will be used (prompt ignored)")
                                .small()
                                .color(egui::Color32::from_rgb(255, 220, 150)),
                        );
                    });
                });
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        // =================================================================
        // Existing Image Section
        // =================================================================
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Input Image").strong());
            ui.label(
                egui::RichText::new("(optional - skips image generation)")
                    .small()
                    .weak(),
            );
        });
        ui.add_space(4.0);

        let mut should_clear_image = false;
        let mut should_show_in_folder: Option<PathBuf> = None;
        if let Some(path) = app.existing_image.clone() {
            // Memoize the label — computing it reads bundle metadata from disk,
            // which we don't want to repeat every frame. Recompute only when the
            // path changes.
            let label = match &app.existing_image_label {
                Some((cached_path, cached_label)) if *cached_path == path => cached_label.clone(),
                _ => {
                    let computed = format_existing_image_label(&path);
                    app.existing_image_label = Some((path.clone(), computed.clone()));
                    computed
                }
            };
            let path = &path;
            let hover = normalize_whitespace_for_display(path);
            let path_buf = PathBuf::from(path);
            ui.horizontal(|ui| {
                // Small thumbnail so the user can eyeball the queued image
                // without opening the file externally. Uses the same
                // file:// URI loader as the preview pane's image tab.
                if path_buf.is_file() {
                    let uri = super::path_to_file_uri(&path_buf);
                    ui.add(
                        egui::Image::new(&uri)
                            .fit_to_exact_size(egui::vec2(56.0, 56.0))
                            .maintain_aspect_ratio(true)
                            .corner_radius(4)
                            .show_loading_spinner(false),
                    );
                }
                ui.label(&label).on_hover_text(&hover);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button(icons::X)
                        .on_hover_text("Clear image")
                        .clicked()
                    {
                        should_clear_image = true;
                    }
                    if ui
                        .small_button(icons::FOLDER_OPEN)
                        .on_hover_text("Show in folder")
                        .clicked()
                        && let Some(parent) = path_buf.parent()
                    {
                        should_show_in_folder = Some(parent.to_path_buf());
                    }
                });
            });
        } else {
            // Dropzone area
            let dropzone_height = 70.0;
            let available_width = ui.available_width();

            let (rect, response) = ui.allocate_exact_size(
                egui::vec2(available_width, dropzone_height),
                egui::Sense::click(),
            );
            app.walkthrough
                .register_rect(WalkthroughStep::ImageDropZone, rect);

            // Handle click on dropzone to open file selector
            if response.clicked() {
                app.select_existing_image();
            }

            // Check for dropped files. Only claim IMAGE files — bundle
            // folders, zips, and bundle.json route to the window-level
            // bundle import (App::handle_bundle_drops), not the input image.
            let mut dropped_image = None;
            ui.ctx().input(|i| {
                for file in &i.raw.dropped_files {
                    if let Some(path) = &file.path
                        && crate::app::App::is_image_file(path)
                    {
                        dropped_image = Some(path.to_string_lossy().to_string());
                        break;
                    }
                }
            });

            // Visual styling for dropzone. Light up only when an IMAGE is
            // being dragged — bundle folders/zips/bundle.json get the
            // window-level import overlay instead, and lighting both reads
            // as "this drop goes to the image slot" when it doesn't.
            let is_being_dragged = ui.ctx().input(|i| {
                i.raw.hovered_files.iter().any(|f| {
                    f.path
                        .as_deref()
                        .is_some_and(crate::app::App::is_image_file)
                })
            });
            let bg_color = if is_being_dragged {
                ui.visuals().selection.bg_fill.gamma_multiply(0.3)
            } else if response.hovered() {
                ui.visuals().extreme_bg_color
            } else {
                egui::Color32::TRANSPARENT
            };

            let stroke_color = if is_being_dragged {
                ui.visuals().selection.stroke.color
            } else {
                ui.visuals().widgets.noninteractive.bg_stroke.color
            };

            // Draw dropzone background with dashed border
            ui.painter().rect(
                rect,
                4,
                bg_color,
                egui::Stroke::NONE,
                egui::StrokeKind::Outside,
            );

            // Draw dashed border
            let dash_len = 6.0;
            let gap_len = 4.0;
            let border_rect = rect.shrink(0.5);

            // Top border
            let mut x = border_rect.left();
            while x < border_rect.right() {
                let end_x = (x + dash_len).min(border_rect.right());
                ui.painter().line_segment(
                    [
                        egui::pos2(x, border_rect.top()),
                        egui::pos2(end_x, border_rect.top()),
                    ],
                    egui::Stroke::new(2.0_f32, stroke_color),
                );
                x += dash_len + gap_len;
            }

            // Bottom border
            x = border_rect.left();
            while x < border_rect.right() {
                let end_x = (x + dash_len).min(border_rect.right());
                ui.painter().line_segment(
                    [
                        egui::pos2(x, border_rect.bottom()),
                        egui::pos2(end_x, border_rect.bottom()),
                    ],
                    egui::Stroke::new(2.0_f32, stroke_color),
                );
                x += dash_len + gap_len;
            }

            // Left border
            let mut y = border_rect.top();
            while y < border_rect.bottom() {
                let end_y = (y + dash_len).min(border_rect.bottom());
                ui.painter().line_segment(
                    [
                        egui::pos2(border_rect.left(), y),
                        egui::pos2(border_rect.left(), end_y),
                    ],
                    egui::Stroke::new(2.0_f32, stroke_color),
                );
                y += dash_len + gap_len;
            }

            // Right border
            y = border_rect.top();
            while y < border_rect.bottom() {
                let end_y = (y + dash_len).min(border_rect.bottom());
                ui.painter().line_segment(
                    [
                        egui::pos2(border_rect.right(), y),
                        egui::pos2(border_rect.right(), end_y),
                    ],
                    egui::Stroke::new(2.0_f32, stroke_color),
                );
                y += dash_len + gap_len;
            }

            // Draw icon and text centered
            let center = rect.center();

            // Image icon
            let icon_color = if is_being_dragged {
                ui.visuals().selection.stroke.color
            } else {
                ui.visuals().weak_text_color()
            };

            ui.painter().text(
                egui::pos2(center.x, center.y - 12.0),
                egui::Align2::CENTER_CENTER,
                icons::IMAGE,
                egui::FontId::proportional(32.0),
                icon_color,
            );

            // Text below icon
            let text_color = if is_being_dragged {
                ui.visuals().strong_text_color()
            } else {
                ui.visuals().weak_text_color()
            };

            ui.painter().text(
                egui::pos2(center.x, center.y + 20.0),
                egui::Align2::CENTER_CENTER,
                if is_being_dragged {
                    "Drop to upload"
                } else {
                    "Drop image here"
                },
                egui::FontId::proportional(12.0),
                text_color,
            );

            ui.add_space(4.0);

            // Buttons below dropzone
            ui.horizontal(|ui| {
                if ui.button(format!("{} Browse...", icons::FOLDER)).clicked() {
                    app.select_existing_image();
                }
                if ui.button(format!("{} Library", icons::BOOK)).clicked() {
                    app.open_library_for_existing_image();
                }
            });
            ui.label(
                egui::RichText::new("(Skip image generation)")
                    .small()
                    .secondary(),
            );

            // Handle dropped file
            if let Some(path) = dropped_image {
                if app.set_existing_image(path.clone()) {
                    app.toasts.push(Toast::success("Image loaded successfully"));
                } else {
                    app.toasts.push(Toast::info(
                        "Invalid file type. Use PNG, JPG, JPEG, or WebP",
                    ));
                }
            }
        }

        if should_clear_image {
            app.clear_existing_image();
        }
        if let Some(folder) = should_show_in_folder {
            crate::app::open_with_system(&folder, Some(&mut app.toasts));
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        // =================================================================
        // Provider & Model Selection Section
        // =================================================================
        let providers_label = ui.label(egui::RichText::new("Providers & Models").strong());
        let providers_top = providers_label.rect.min.y;
        ui.add_space(4.0);

        // Use cached provider registry
        let available_providers = app.provider_registry.list_available();

        // Track if providers changed to update models
        let old_image_provider = app.image_provider.clone();
        let old_3d_provider = app.model_3d_provider.clone();

        // If no providers available, show warning
        if available_providers.is_empty() {
            ui.colored_label(
                egui::Color32::from_rgb(255, 180, 100),
                format!("{} No providers available", icons::WARNING),
            );
            ui.label(
                egui::RichText::new("Configure API keys in Settings to enable generation")
                    .small()
                    .secondary(),
            );
            ui.add_space(4.0);
        }

        // Image Generation Section — fully disabled (but visible) when the
        // user has supplied an existing image, since the pipeline will skip
        // text-to-image. Mirrors the 3D section's behavior when skip_3d is on.
        let image_gen_enabled = !has_existing_image;
        ui.add_enabled_ui(image_gen_enabled, |ui| {
            ui.label(egui::RichText::new("Image Generation").size(13.0).strong());
            ui.add_space(2.0);

            let image_provider_list: Vec<_> = available_providers
                .iter()
                .filter(|p| p.supports(ProviderCapability::TextToImage))
                .collect();

            egui::Grid::new("image_generation_selectors")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    // Image provider selector
                    let current_image_provider = image_provider_list
                        .iter()
                        .find(|p| p.id() == app.image_provider.as_str())
                        .map(|p| p.name())
                        .unwrap_or(&app.image_provider);

                    ui.label("Provider:");
                    egui::ComboBox::from_id_salt("image_provider_selector")
                        .selected_text(current_image_provider)
                        .show_ui(ui, |ui| {
                            for provider in &image_provider_list {
                                let provider_id = provider.id();
                                if ui
                                    .add(egui::Button::selectable(
                                        app.image_provider == provider_id,
                                        provider.name(),
                                    ))
                                    .on_hover_text(provider.metadata().description.clone())
                                    .clicked()
                                {
                                    app.image_provider = provider_id.to_string();
                                }
                            }
                        });
                    ui.end_row();

                    // Image model selector
                    if let Some(img_provider) = app.provider_registry.get(&app.image_provider) {
                        // If provider changed, update model to default
                        if old_image_provider != app.image_provider
                            && let Ok(default_img) =
                                img_provider.get_default_model(ProviderCapability::TextToImage)
                        {
                            app.image_model = default_img.id;
                        }

                        let image_models =
                            img_provider.list_models(ProviderCapability::TextToImage);
                        let current_image_model =
                            image_models.iter().find(|m| m.id == app.image_model);

                        ui.label("Model:");
                        egui::ComboBox::from_id_salt("image_model_selector")
                            .selected_text(
                                current_image_model
                                    .map(|m| format_model_display_name(&m.name, &m.id))
                                    .unwrap_or_else(|| app.image_model.clone()),
                            )
                            .show_ui(ui, |ui| {
                                for model in &image_models {
                                    let display_name =
                                        format_model_display_name(&model.name, &model.id);
                                    if ui
                                        .add(egui::Button::selectable(
                                            app.image_model == model.id,
                                            display_name,
                                        ))
                                        .on_hover_text(model.description.as_deref().unwrap_or(""))
                                        .clicked()
                                    {
                                        app.image_model = model.id.clone();
                                    }
                                }
                            });
                        ui.end_row();
                    }
                });

            // Image model settings (directly under image model selector)
            render_image_model_settings(ui, app, &old_image_provider);
        }); // end Image Generation add_enabled_ui

        ui.add_space(8.0);

        // 3D Generation Section — fully disabled (but visible) when image-only.
        // Keeping the controls visible with a grayed-out appearance makes the
        // mode change feel intentional rather than hiding UI.
        //
        // The image-only toggle lives here rather than under Post-Processing:
        // it decides whether this stage runs at all, which is pipeline scope,
        // not something applied to a finished model. It also has to stay
        // outside the add_enabled_ui below, or turning it on would disable the
        // control needed to turn it back off.
        ui.label(egui::RichText::new("3D Generation").size(13.0).strong());
        ui.add_space(2.0);

        // Stays interactive even when it conflicts with an input image. It was
        // previously disabled in that case, which trapped anyone who ticked it
        // before choosing the image: the box stayed checked with no way to
        // clear it. Both selections are the user's; the app reports the
        // conflict below instead of resolving it for them.
        ui.checkbox(&mut app.skip_3d, "Image only (skip 3D)")
            .on_hover_text("Stop after image generation. No GLB or FBX will be produced.");

        // An input image replaces image generation and this skips 3D, so
        // together they leave no stage to run. Say so where the choice was
        // made — the Generate button is disabled, but a grayed-out button at
        // the bottom of the sidebar doesn't explain itself.
        if app.skip_3d && has_existing_image {
            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(
                    egui::Color32::from_rgb(255, 180, 100),
                    format!("{} Nothing to generate", icons::WARNING),
                );
            });
            ui.label(
                egui::RichText::new(
                    "The input image already replaces image generation. Clear the image \
                     or uncheck this to continue.",
                )
                .size(11.0)
                .weak(),
            );
        }
        ui.add_space(4.0);

        let three_d_enabled = !app.skip_3d;
        ui.add_enabled_ui(three_d_enabled, |ui| {
            let model_3d_provider_list: Vec<_> = available_providers
                .iter()
                .filter(|p| p.supports(ProviderCapability::ImageTo3D))
                .collect();

            egui::Grid::new("3d_generation_selectors")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    // 3D provider selector
                    let current_3d_provider = model_3d_provider_list
                        .iter()
                        .find(|p| p.id() == app.model_3d_provider.as_str())
                        .map(|p| p.name())
                        .unwrap_or(&app.model_3d_provider);

                    ui.label("Provider:");
                    egui::ComboBox::from_id_salt("3d_provider_selector")
                        .selected_text(current_3d_provider)
                        .show_ui(ui, |ui| {
                            for provider in &model_3d_provider_list {
                                let provider_id = provider.id();
                                if ui
                                    .add(egui::Button::selectable(
                                        app.model_3d_provider == provider_id,
                                        provider.name(),
                                    ))
                                    .on_hover_text(provider.metadata().description.clone())
                                    .clicked()
                                {
                                    app.model_3d_provider = provider_id.to_string();
                                }
                            }
                        });
                    ui.end_row();

                    // 3D model selector
                    if let Some(model_3d_provider) =
                        app.provider_registry.get(&app.model_3d_provider)
                    {
                        // If provider changed, update model to default
                        if old_3d_provider != app.model_3d_provider
                            && let Ok(default_3d) =
                                model_3d_provider.get_default_model(ProviderCapability::ImageTo3D)
                        {
                            app.model_3d = default_3d.id;
                        }

                        let model_3d_models =
                            model_3d_provider.list_models(ProviderCapability::ImageTo3D);
                        let current_3d_model =
                            model_3d_models.iter().find(|m| m.id == app.model_3d);

                        ui.label("Model:");
                        egui::ComboBox::from_id_salt("3d_model_selector")
                            .selected_text(
                                current_3d_model
                                    .map(|m| format_model_display_name(&m.name, &m.id))
                                    .unwrap_or_else(|| app.model_3d.clone()),
                            )
                            .show_ui(ui, |ui| {
                                for model in &model_3d_models {
                                    let display_name =
                                        format_model_display_name(&model.name, &model.id);
                                    if ui
                                        .add(egui::Button::selectable(
                                            app.model_3d == model.id,
                                            display_name,
                                        ))
                                        .on_hover_text(model.description.as_deref().unwrap_or(""))
                                        .clicked()
                                    {
                                        app.model_3d = model.id.clone();
                                    }
                                }
                            });
                        ui.end_row();
                    }
                });

            // 3D model settings (directly under 3D model selector)
            render_3d_model_settings(ui, app, &old_3d_provider);
        }); // end 3D Generation add_enabled_ui

        // Persist model selections when changed
        let selections_changed = app.app_state.selected_image_provider.as_deref()
            != Some(&app.image_provider)
            || app.app_state.selected_image_model.as_deref() != Some(&app.image_model)
            || app.app_state.selected_3d_provider.as_deref() != Some(&app.model_3d_provider)
            || app.app_state.selected_3d_model.as_deref() != Some(&app.model_3d);
        if selections_changed {
            app.app_state.selected_image_provider = Some(app.image_provider.clone());
            app.app_state.selected_image_model = Some(app.image_model.clone());
            app.app_state.selected_3d_provider = Some(app.model_3d_provider.clone());
            app.app_state.selected_3d_model = Some(app.model_3d.clone());
            let _ = app.app_state.save();
        }

        // Register providers section rect for walkthrough (from heading to here)
        let providers_rect = egui::Rect::from_min_max(
            egui::pos2(ui.max_rect().left(), providers_top),
            egui::pos2(ui.max_rect().right(), ui.cursor().top()),
        );
        app.walkthrough
            .register_rect(WalkthroughStep::ProvidersSection, providers_rect);

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        // =================================================================
        // Post-Processing Section
        // =================================================================
        ui.label(egui::RichText::new("Post-Processing").strong());
        ui.add_space(4.0);

        // FBX toggle with Blender availability check
        let prev_fbx = app.export_fbx;
        ui.add_enabled_ui(!app.skip_3d, |ui| {
            ui.checkbox(&mut app.export_fbx, "Export FBX (requires Blender)");
        });
        if app.export_fbx != prev_fbx {
            app.settings.export_fbx_default = app.export_fbx;
            if let Err(e) = app.settings.save() {
                tracing::error!("Failed to save FBX setting: {}", e);
            }
        }

        // Show warning if FBX is enabled but Blender is not available
        if app.export_fbx && !app.blender_available {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.colored_label(
                    egui::Color32::from_rgb(255, 180, 100),
                    format!("{} Blender not found", icons::WARNING),
                );
            });
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                ui.label(egui::RichText::new("Install from:").small().secondary());
                if ui.link("blender.org/download").clicked() {
                    crate::app::open_with_system("https://www.blender.org/download/", None);
                }
            });
        }

        ui.add_space(12.0);
        ui.separator();
        ui.add_space(4.0);

        // =================================================================
        // Generate Button
        // =================================================================
        let can_generate = app.can_generate();
        let is_running = app.state.lock().unwrap().running;

        ui.vertical_centered(|ui| {
            if is_running {
                // Cancel button when pipeline is running
                let cancel_button = egui::Button::new(
                    egui::RichText::new(format!("{} Cancel", icons::CIRCLE_XMARK))
                        .size(16.0)
                        .color(egui::Color32::WHITE),
                )
                .fill(egui::Color32::from_rgb(150, 50, 50))
                .min_size(egui::vec2(200.0, 40.0));

                let button_response = ui.add(cancel_button);
                app.walkthrough
                    .register_rect(WalkthroughStep::GenerateButton, button_response.rect);

                if button_response.clicked() {
                    app.cancel_pipeline();
                }
            } else {
                // Generate button when idle
                let button = egui::Button::new(egui::RichText::new("🚀 Generate").size(16.0))
                    .min_size(egui::vec2(200.0, 40.0));

                let mut button_response = ui.add_enabled(can_generate, button);
                app.walkthrough
                    .register_rect(WalkthroughStep::GenerateButton, button_response.rect);

                if !can_generate && let Some(reason) = app.generate_disabled_reason() {
                    button_response = button_response.on_disabled_hover_text(reason);
                }

                if button_response.clicked() {
                    app.run_pipeline();
                }
            }
        });

        ui.add_space(8.0);
    });
}

fn truncate_path(path: &str, max_len: usize) -> String {
    let char_count = path.chars().count();
    if char_count <= max_len {
        path.to_string()
    } else {
        let skip = char_count - (max_len.saturating_sub(3));
        let suffix: String = path.chars().skip(skip).collect();
        format!("...{}", suffix)
    }
}

/// Replace Unicode whitespace that our font doesn't render (e.g. the
/// narrow no-break space macOS puts between time and AM/PM in screenshot
/// filenames) with a regular space. Without this, users see tofu glyphs
/// for filenames we had nothing to do with.
fn normalize_whitespace_for_display(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect()
}

/// Build a short human-readable label for an existing-image path.
///
/// For bundle images (`<output>/<timestamp>/image.png`), prefer the bundle's
/// `name`, then first line of its `prompt`, then the timestamp folder name —
/// anything but the generic filename. For arbitrary filesystem images (e.g.
/// a screenshot from Downloads), show the filename, not the folder.
fn format_existing_image_label(path: &str) -> String {
    use std::path::Path;
    let p = Path::new(path);
    if let Some(parent) = p.parent()
        && let Ok(Some(metadata)) = asset_tap_core::bundle::BundleMetadata::load(parent)
    {
        if let Some(name) = metadata.name.as_deref().filter(|s| !s.is_empty()) {
            return truncate_path(&normalize_whitespace_for_display(name), 30);
        }
        if let Some(prompt) = metadata
            .config
            .as_ref()
            .and_then(|c| c.prompt.as_deref())
            .filter(|s| !s.is_empty())
        {
            // First line of the prompt, truncated.
            let first_line = prompt.lines().next().unwrap_or(prompt);
            return truncate_path(&normalize_whitespace_for_display(first_line), 30);
        }
        // Bundle exists but has no name/prompt — use its timestamp folder.
        if let Some(folder) = parent.file_name().and_then(|s| s.to_str()) {
            return normalize_whitespace_for_display(folder);
        }
    }
    // Not a bundle — show the filename so arbitrary picks from disk stay
    // recognizable (e.g. "photo.png" instead of "Downloads").
    p.file_name()
        .and_then(|s| s.to_str())
        .map(|s| truncate_path(&normalize_whitespace_for_display(s), 30))
        .unwrap_or_else(|| truncate_path(&normalize_whitespace_for_display(path), 30))
}

/// Format a model's display name with a distinguishing identifier.
///
/// For models with similar names, this adds a shortened version of the model ID
/// to help users distinguish between them. For example:
/// - "Hunyuan3d V3 (rapid)" vs "Hunyuan3d V3 (standard)"
///
/// This function is provider-agnostic and works by identifying the most specific
/// non-generic part of the model ID.
fn format_model_display_name(name: &str, id: &str) -> String {
    // Extract the last meaningful part of the model ID
    // Examples (provider-agnostic):
    //   "provider/hunyuan-3d/v3.1/pro/image-to-3d" -> "pro"
    //   "provider/sam-3/image-to-3d" -> "sam-3"
    //   "trellis-2" -> "trellis-2"
    //   "openai/gpt-4" -> "gpt-4"

    let id_parts: Vec<&str> = id.split('/').collect();

    // Generic capability suffixes to skip (not provider-specific)
    let generic_suffixes = [
        "image-to-3d",
        "text-to-image",
        "text-to-video",
        "image-to-video",
    ];

    // Try to find a distinguishing part that's not generic
    let distinguisher = id_parts
        .iter()
        .rev()
        .find(|part| {
            // Skip generic capability suffixes
            let is_generic_suffix = generic_suffixes.iter().any(|suffix| *part == suffix);

            // Skip empty parts
            let is_empty = part.is_empty();

            // Skip version-only segments (e.g., "v3.1", "v2")
            // Must be at least 2 chars to avoid skipping valid single-char parts
            let is_version_only = part.len() >= 2
                && part.starts_with('v')
                && part[1..].chars().all(|c| c.is_numeric() || c == '.');

            // Keep this part if it's not generic and we have multiple parts
            (!is_empty && !is_generic_suffix && !is_version_only) || id_parts.len() == 1
        })
        .copied()
        .unwrap_or(id_parts.last().unwrap_or(&id));

    // If the distinguisher is already in the name, don't add it
    if name.to_lowercase().contains(&distinguisher.to_lowercase()) {
        name.to_string()
    } else {
        format!("{} ({})", name, distinguisher)
    }
}

/// Open the template editor in view mode for the given template.
fn open_template_for_viewing(app: &mut App, name: &str) {
    if let Some(template) = asset_tap_core::templates::get_template_definition(name) {
        app.editing_template = Some(template.clone());
        app.editor_name_input = template.name.clone();
        app.editor_description_input = template.description.clone();
        app.editor_template_input = template.template.clone();
        app.editor_mode = crate::app::TemplateEditorMode::ViewOnly;
        app.show_template_editor = true;
    }
}

/// Open the template editor in create mode with empty fields.
fn open_template_for_creation(app: &mut App) {
    app.editing_template = None;
    app.editor_name_input.clear();
    app.editor_description_input.clear();
    app.editor_template_input = "A ${description}.".to_string(); // Starter template
    app.editor_mode = crate::app::TemplateEditorMode::Create;
    app.show_template_editor = true;
}

/// Delete a custom template and update the app state.
fn delete_custom_template(app: &mut App, name: &str) {
    match asset_tap_core::templates::delete_custom_template(name) {
        Ok(_) => {
            // Refresh available templates
            app.available_templates = asset_tap_core::templates::list_templates();

            // Deselect if this was the selected template
            if app.template.as_deref() == Some(name) {
                app.template = None;
            }

            // Show success toast
            app.toasts
                .push(Toast::success("Template deleted successfully"));
        }
        Err(e) => {
            app.toasts
                .push(Toast::info(format!("Failed to delete: {}", e)));
        }
    }
}

// =============================================================================
// Model Settings (per-model tunable parameters)
// =============================================================================

/// Build a composite key for storing per-model parameter overrides.
fn model_params_key(provider_id: &str, model_id: &str) -> String {
    format!("{}/{}", provider_id, model_id)
}

/// Render image model settings panel directly under the image model selector.
fn render_image_model_settings(ui: &mut egui::Ui, app: &mut App, old_provider: &str) {
    let provider_changed = old_provider != app.image_provider;
    let model_changed = app.app_state.selected_image_model.as_deref() != Some(&app.image_model);
    render_model_settings_panel(
        ui,
        app,
        provider_changed || model_changed,
        &app.image_provider.clone(),
        &app.image_model.clone(),
        ProviderCapability::TextToImage,
        "image_model_settings",
        "Image Model Settings",
        |app| &mut app.image_model_params,
    );
}

/// Render 3D model settings panel directly under the 3D model selector.
fn render_3d_model_settings(ui: &mut egui::Ui, app: &mut App, old_provider: &str) {
    let provider_changed = old_provider != app.model_3d_provider;
    let model_changed = app.app_state.selected_3d_model.as_deref() != Some(&app.model_3d);
    render_model_settings_panel(
        ui,
        app,
        provider_changed || model_changed,
        &app.model_3d_provider.clone(),
        &app.model_3d.clone(),
        ProviderCapability::ImageTo3D,
        "3d_model_settings",
        "3D Model Settings",
        |app| &mut app.model_3d_params,
    );
}

/// Shared implementation for rendering a model settings panel.
///
/// Loads saved params on selection change, collects parameter definitions from
/// the registry, renders the collapsible panel, and persists changes.
#[allow(clippy::too_many_arguments)]
fn render_model_settings_panel(
    ui: &mut egui::Ui,
    app: &mut App,
    selection_changed: bool,
    provider_id: &str,
    model_id: &str,
    capability: ProviderCapability,
    panel_id: &str,
    panel_label: &str,
    get_params: fn(&mut App) -> &mut std::collections::HashMap<String, serde_json::Value>,
) {
    let key = model_params_key(provider_id, model_id);

    // Load saved params when provider/model changes
    if selection_changed {
        *get_params(app) = app
            .app_state
            .model_parameters
            .get(&key)
            .cloned()
            .unwrap_or_default();
    }

    // Collect parameter definitions (immutable borrow ends before mutable use)
    let param_defs: Vec<asset_tap_core::providers::ParameterDef> = app
        .provider_registry
        .get(provider_id)
        .and_then(|p| {
            p.list_models(capability)
                .into_iter()
                .find(|m| m.id == model_id)
        })
        .map(|m| m.parameters)
        .unwrap_or_default();

    if !param_defs.is_empty() {
        ui.add_space(4.0);
        let params = get_params(app);
        let changed = render_parameter_panel(ui, panel_id, panel_label, &param_defs, params);
        if changed {
            let params_clone = get_params(app).clone();
            app.app_state.model_parameters.insert(key, params_clone);
            let _ = app.app_state.save();
        }
    }
}

/// Render a collapsible panel with parameter widgets.
///
/// Returns true if any parameter value was changed.
fn render_parameter_panel(
    ui: &mut egui::Ui,
    id: &str,
    label: &str,
    parameters: &[asset_tap_core::providers::ParameterDef],
    values: &mut std::collections::HashMap<String, serde_json::Value>,
) -> bool {
    let mut changed = false;

    egui::CollapsingHeader::new(egui::RichText::new(label).size(12.0).weak())
        .id_salt(id)
        .default_open(false)
        .show(ui, |ui| {
            for param in parameters {
                changed |= render_parameter_widget(ui, param, values);
            }

            ui.add_space(4.0);
            if ui
                .small_button("Reset to Defaults")
                .on_hover_text("Clear all overrides and use YAML defaults")
                .clicked()
            {
                values.clear();
                changed = true;
            }
        });

    changed
}

#[derive(Copy, Clone)]
enum NumericKind {
    Integer,
    Float,
}

/// Dropdown entry shown for a cleared (`allow_unset`) select parameter.
///
/// Parenthesized so it can't collide with a real option value — provider
/// options are API enum values, none of which are wrapped in parentheses.
const UNSET_LABEL: &str = "(unset)";

/// Map a chosen dropdown label back to the JSON value to store.
///
/// Options are rendered as strings but may be numbers in YAML (trellis-2's
/// `resolution: [512, 1024, 1536]`), so the original type is recovered from
/// the options list rather than reconstructed from the label. The `(unset)`
/// entry maps to null, which strips the key from the request body entirely.
fn select_value_for_label(
    param: &asset_tap_core::providers::ParameterDef,
    label: &str,
) -> serde_json::Value {
    if param.allow_unset && label == UNSET_LABEL {
        return serde_json::Value::Null;
    }
    param
        .options
        .as_ref()
        .and_then(|opts| {
            opts.iter()
                .find(|o| json_scalar_display(o) == label)
                .cloned()
        })
        .unwrap_or_else(|| serde_json::Value::String(label.to_string()))
}

/// Human-readable form of a JSON scalar (strings, numbers, booleans) for
/// rendering as combo-box option labels.
fn json_scalar_display(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        _ => v.to_string(),
    }
}

/// Render a typed numeric text input for wide-range fields where a slider is
/// impractical (e.g. 40k–1.5M polycounts) or where "unset" is a valid state
/// (e.g. seeds — empty field sends null so the provider picks a random seed).
fn render_numeric_input(
    ui: &mut egui::Ui,
    param: &asset_tap_core::providers::ParameterDef,
    values: &mut std::collections::HashMap<String, serde_json::Value>,
    kind: NumericKind,
) -> bool {
    let mut changed = false;
    let stored = values.get(&param.name);

    // Resolve the string shown in the field. A stored null = user cleared it.
    let mut text = match stored {
        Some(v) if v.is_null() => String::new(),
        Some(v) => match kind {
            NumericKind::Integer => v
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| v.as_f64().map(|f| f.to_string()).unwrap_or_default()),
            NumericKind::Float => v.as_f64().map(|f| f.to_string()).unwrap_or_default(),
        },
        None => match kind {
            NumericKind::Integer => param
                .default
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_default(),
            NumericKind::Float => param
                .default
                .as_f64()
                .map(|f| f.to_string())
                .unwrap_or_default(),
        },
    };

    let before = text.clone();
    ui.label(&param.label);

    const NUMERIC_INPUT_WIDTH: f32 = 90.0;
    let response = ui.add(
        egui::TextEdit::singleline(&mut text)
            .desired_width(NUMERIC_INPUT_WIDTH)
            .hint_text(match kind {
                NumericKind::Integer => "integer",
                NumericKind::Float => "number",
            }),
    );
    if let Some(ref desc) = param.description {
        response.on_hover_text(desc);
    }

    // Display range hint next to the input so users know valid bounds
    if let (Some(min), Some(max)) = (param.min, param.max) {
        ui.label(
            egui::RichText::new(match kind {
                NumericKind::Integer => format!("({}–{})", min as i64, max as i64),
                NumericKind::Float => format!("({}–{})", min, max),
            })
            .size(10.0)
            .weak(),
        );
    }

    if text == before {
        return changed;
    }

    let trimmed = text.trim();
    if trimmed.is_empty() {
        values.insert(param.name.clone(), serde_json::Value::Null);
        changed = true;
        return changed;
    }

    match kind {
        NumericKind::Integer => {
            if let Ok(n) = trimmed.parse::<i64>() {
                let clamped = param.clamp_f64(n as f64) as i64;
                values.insert(param.name.clone(), serde_json::Value::from(clamped));
                changed = true;
            }
            // On parse failure, keep prior value — don't write garbage
        }
        NumericKind::Float => {
            if let Ok(n) = trimmed.parse::<f64>() {
                let clamped = param.clamp_f64(n);
                values.insert(param.name.clone(), serde_json::Value::from(clamped));
                changed = true;
            }
        }
    }

    changed
}

/// Render a single parameter widget based on its type.
///
/// Returns true if the value was changed.
fn render_parameter_widget(
    ui: &mut egui::Ui,
    param: &asset_tap_core::providers::ParameterDef,
    values: &mut std::collections::HashMap<String, serde_json::Value>,
) -> bool {
    use asset_tap_core::providers::ParameterType;

    let mut changed = false;

    use asset_tap_core::providers::ParameterWidget;
    let use_input = param.widget == Some(ParameterWidget::Input);

    ui.horizontal(|ui| match param.param_type {
        ParameterType::Float if use_input => {
            changed |= render_numeric_input(ui, param, values, NumericKind::Float);
        }
        ParameterType::Integer if use_input => {
            changed |= render_numeric_input(ui, param, values, NumericKind::Integer);
        }
        ParameterType::Float => {
            let default = param.default.as_f64().unwrap_or(0.0);
            let min = param.min.unwrap_or(0.0);
            let max = param.max.unwrap_or(100.0);
            let step = param.step.unwrap_or(0.1);

            let current = values
                .get(&param.name)
                .and_then(|v| v.as_f64())
                .unwrap_or(default);
            let mut val = current;

            ui.label(&param.label);
            let slider = egui::Slider::new(&mut val, min..=max)
                .step_by(step)
                .min_decimals(1);
            let response = ui.add(slider);
            if let Some(ref desc) = param.description {
                response.on_hover_text(desc);
            }

            if (val - current).abs() > f64::EPSILON {
                values.insert(param.name.clone(), serde_json::Value::from(val));
                changed = true;
            }
        }
        ParameterType::Integer => {
            let default = param.default.as_i64().unwrap_or(0);
            let min = param.min.unwrap_or(0.0) as i64;
            let max = param.max.unwrap_or(100.0) as i64;

            let current = values
                .get(&param.name)
                .and_then(|v| v.as_i64())
                .unwrap_or(default);
            let mut val = current;

            ui.label(&param.label);
            let response = ui.add(egui::Slider::new(&mut val, min..=max));
            if let Some(ref desc) = param.description {
                response.on_hover_text(desc);
            }

            if val != current {
                values.insert(param.name.clone(), serde_json::Value::from(val));
                changed = true;
            }
        }
        ParameterType::Boolean => {
            let default = param.default.as_bool().unwrap_or(false);
            let current = values
                .get(&param.name)
                .and_then(|v| v.as_bool())
                .unwrap_or(default);
            let mut val = current;

            let response = ui.checkbox(&mut val, &param.label);
            if let Some(ref desc) = param.description {
                response.on_hover_text(desc);
            }

            if val != current {
                values.insert(param.name.clone(), serde_json::Value::from(val));
                changed = true;
            }
        }
        ParameterType::String => {
            let default = param.default.as_str().unwrap_or("").to_string();
            let current = values
                .get(&param.name)
                .and_then(|v| v.as_str())
                .unwrap_or(&default)
                .to_string();
            let mut val = current.clone();

            ui.label(&param.label);
            let response = ui.text_edit_singleline(&mut val);
            if let Some(ref desc) = param.description {
                response.on_hover_text(desc);
            }

            if val != current {
                // `Input` means an empty field omits the key rather than
                // sending "", matching the numeric inputs. Optional text fields
                // like Meshy's texture_prompt document no empty-string value.
                let new_value = if use_input && val.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::Value::from(val)
                };
                values.insert(param.name.clone(), new_value);
                changed = true;
            }
        }
        ParameterType::Select => {
            // Options can be strings OR numbers (e.g. trellis-2's
            // `resolution: [512, 1024, 1536]`). Render as strings for display
            // but preserve the original JSON type on write so the runtime
            // sends the correct type to the API.
            let current_value = values
                .get(&param.name)
                .cloned()
                .unwrap_or(param.default.clone());
            let current_str = if current_value.is_null() {
                UNSET_LABEL.to_string()
            } else {
                json_scalar_display(&current_value)
            };
            let mut selected_str = current_str.clone();

            ui.label(&param.label);
            let combo = egui::ComboBox::from_id_salt(&param.name).selected_text(&selected_str);
            let response = combo.show_ui(ui, |ui| {
                // Every entry writes one of `options`, so a dropdown can't
                // express "no value" without this. Parameters that are mutually
                // exclusive with another need it; the CLI equivalent is
                // `--param name=`.
                if param.allow_unset {
                    ui.selectable_value(&mut selected_str, UNSET_LABEL.to_string(), UNSET_LABEL);
                }
                if let Some(ref options) = param.options {
                    for opt in options {
                        let opt_str = json_scalar_display(opt);
                        ui.selectable_value(&mut selected_str, opt_str.clone(), &opt_str);
                    }
                }
            });
            if let Some(ref desc) = param.description {
                response.response.on_hover_text(desc);
            }

            if selected_str != current_str {
                values.insert(
                    param.name.clone(),
                    select_value_for_label(param, &selected_str),
                );
                changed = true;
            }
        }
    });

    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use asset_tap_core::providers::ParameterDef;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn normalize_whitespace_replaces_narrow_nbsp() {
        // U+202F narrow no-break space (macOS screenshot filenames between
        // time and AM/PM) → regular space so the font renders it.
        let narrow_nbsp = "9.05.20\u{202F}PM";
        assert_eq!(normalize_whitespace_for_display(narrow_nbsp), "9.05.20 PM");
    }

    #[test]
    fn normalize_whitespace_preserves_printable_chars() {
        let normal = "hello world";
        assert_eq!(normalize_whitespace_for_display(normal), "hello world");
    }

    /// Build a bundle dir with a bundle.json for the given name/prompt.
    /// Uses Option for both so tests can exercise the full fallback chain
    /// (name > prompt > folder name) without hand-rolling JSON each time.
    fn write_bundle(dir: &std::path::Path, name: Option<&str>, prompt: Option<&str>) {
        let md = asset_tap_core::bundle::BundleMetadata {
            name: name.map(String::from),
            config: prompt.map(|p| asset_tap_core::history::GenerationConfig {
                prompt: Some(p.to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        md.save(dir).unwrap();
        fs::write(dir.join("image.png"), b"fake").unwrap();
    }

    #[test]
    fn label_prefers_bundle_name() {
        let tmp = TempDir::new().unwrap();
        let bundle = tmp.path().join("2026-04-19_123456");
        fs::create_dir(&bundle).unwrap();
        write_bundle(&bundle, Some("My Cool Robot"), Some("a robot"));
        let image = bundle.join("image.png");
        let label = format_existing_image_label(image.to_str().unwrap());
        assert_eq!(label, "My Cool Robot");
    }

    #[test]
    fn label_falls_back_to_prompt_when_no_name() {
        let tmp = TempDir::new().unwrap();
        let bundle = tmp.path().join("2026-04-19_123456");
        fs::create_dir(&bundle).unwrap();
        write_bundle(&bundle, None, Some("first line\nsecond line"));
        let image = bundle.join("image.png");
        let label = format_existing_image_label(image.to_str().unwrap());
        assert_eq!(label, "first line");
    }

    #[test]
    fn label_falls_back_to_folder_when_bundle_has_no_name_or_prompt() {
        let tmp = TempDir::new().unwrap();
        let bundle = tmp.path().join("2026-04-19_123456");
        fs::create_dir(&bundle).unwrap();
        write_bundle(&bundle, None, None);
        let image = bundle.join("image.png");
        let label = format_existing_image_label(image.to_str().unwrap());
        assert_eq!(label, "2026-04-19_123456");
    }

    #[test]
    fn label_uses_filename_for_non_bundle_paths() {
        // No bundle.json next to the image → show the filename, not the
        // parent directory. This keeps random picks from disk (e.g. a
        // screenshot in ~/Downloads) recognizable.
        let tmp = TempDir::new().unwrap();
        let image = tmp.path().join("photo.png");
        fs::write(&image, b"fake").unwrap();
        let label = format_existing_image_label(image.to_str().unwrap());
        assert_eq!(label, "photo.png");
    }

    fn select_def(allow_unset: bool, options: Vec<serde_json::Value>) -> ParameterDef {
        ParameterDef {
            name: "aspect_ratio".into(),
            label: "Aspect Ratio".into(),
            description: None,
            param_type: asset_tap_core::providers::ParameterType::Select,
            default: serde_json::json!("1:1"),
            min: None,
            max: None,
            step: None,
            options: Some(options),
            widget: None,
            allow_unset,
        }
    }

    #[test]
    fn select_unset_entry_clears_to_null() {
        // Meshy rejects aspect_ratio alongside generate_multi_view, so the GUI
        // needs the same escape hatch the CLI has in `--param aspect_ratio=`.
        let param = select_def(
            true,
            vec![serde_json::json!("1:1"), serde_json::json!("3:4")],
        );
        assert_eq!(
            select_value_for_label(&param, UNSET_LABEL),
            serde_json::Value::Null
        );
        assert_eq!(
            select_value_for_label(&param, "3:4"),
            serde_json::json!("3:4")
        );
    }

    #[test]
    fn select_preserves_numeric_option_types() {
        // trellis-2's resolution options are numbers; writing back the label as
        // a string would send the API the wrong type.
        let param = select_def(false, vec![serde_json::json!(512), serde_json::json!(1024)]);
        assert_eq!(
            select_value_for_label(&param, "1024"),
            serde_json::json!(1024)
        );
    }

    #[test]
    fn unset_label_is_literal_when_not_allowed() {
        // Without allow_unset there is no "(unset)" entry to pick, so the label
        // must not be treated as a magic clear value.
        let param = select_def(false, vec![serde_json::json!("1:1")]);
        assert_ne!(
            select_value_for_label(&param, UNSET_LABEL),
            serde_json::Value::Null
        );
    }

    #[test]
    fn label_normalizes_whitespace_in_filename() {
        // macOS screenshot filenames contain U+202F between time and AM/PM.
        // We fold it to a regular space so the font doesn't show tofu.
        let tmp = TempDir::new().unwrap();
        let image = tmp.path().join("Screenshot at 9.05.20\u{202F}PM.png");
        fs::write(&image, b"fake").unwrap();
        let label = format_existing_image_label(image.to_str().unwrap());
        assert!(!label.contains('\u{202F}'));
        assert!(label.contains("9.05.20 PM"));
    }
}
