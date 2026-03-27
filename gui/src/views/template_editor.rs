//! Template editor modal for creating and customizing templates.

use crate::app::{App, TemplateEditorMode};
use eframe::egui;

/// Show the template editor modal if open.
pub fn show_template_editor(ctx: &egui::Context, app: &mut App) {
    if !app.show_template_editor {
        return;
    }

    let is_readonly = app.editor_mode == TemplateEditorMode::ViewOnly
        && app.editing_template.as_ref().is_some_and(|t| t.is_builtin);

    let window_title = if is_readonly {
        "View Template"
    } else if app.editing_template.is_some() {
        "Edit Template"
    } else {
        "Create Template"
    };

    let mut open = true;
    let mut clicked_outside = false;

    // Semi-transparent backdrop that can be clicked to close
    let modal_id = egui::Id::new("template_editor_modal");
    egui::Area::new(modal_id.with("backdrop"))
        .fixed_pos(egui::pos2(0.0, 0.0))
        .order(egui::Order::Background)
        .show(ctx, |ui| {
            let screen_rect = ctx.screen_rect();
            let response = ui.allocate_response(screen_rect.size(), egui::Sense::click());
            if response.clicked() {
                clicked_outside = true;
            }
            // Draw semi-transparent backdrop
            ui.painter()
                .rect_filled(screen_rect, 0, egui::Color32::from_black_alpha(128));
        });

    egui::Window::new(window_title)
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(600.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.vertical(|ui| {
                // Template name input
                ui.label("Template Name:");
                ui.add_enabled(
                    !is_readonly,
                    egui::TextEdit::singleline(&mut app.editor_name_input)
                        .hint_text("e.g., creature, vehicle"),
                );

                ui.add_space(8.0);

                // Template description input
                ui.label("Description:");
                ui.add_enabled(
                    !is_readonly,
                    egui::TextEdit::multiline(&mut app.editor_description_input)
                        .desired_rows(2)
                        .hint_text("Describe what this template is optimized for..."),
                );

                ui.add_space(8.0);

                // Template syntax input with help text
                ui.label("Template Syntax:");
                ui.small("Use ${description} as a placeholder for user input");

                // Large multiline editor for template
                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .show(ui, |ui| {
                        ui.add_enabled(
                            !is_readonly,
                            egui::TextEdit::multiline(&mut app.editor_template_input)
                                .desired_rows(8)
                                .code_editor()
                                .desired_width(f32::INFINITY),
                        );
                    });

                ui.add_space(8.0);

                // Preview section
                ui.separator();
                ui.label("Preview:");
                ui.small("With example: 'cowboy ninja'");

                let preview = app
                    .editor_template_input
                    .replace("${description}", "cowboy ninja");

                ui.group(|ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::vertical()
                        .max_height(80.0)
                        .show(ui, |ui| {
                            ui.label(preview);
                        });
                });

                ui.add_space(8.0);

                // Error display
                if let Some(error) = &app.editor_error {
                    ui.colored_label(egui::Color32::from_rgb(220, 60, 60), error);
                    ui.add_space(8.0);
                }

                // Action buttons
                ui.separator();
                ui.horizontal(|ui| {
                    if !is_readonly && ui.button("Save Template").clicked() {
                        handle_save(app);
                    }

                    if is_readonly && ui.button("Duplicate to Customize").clicked() {
                        // Switch to create mode with duplicated content
                        app.editor_mode = TemplateEditorMode::Create;
                        app.editing_template = None;
                        app.editor_name_input = format!("{} (Copy)", app.editor_name_input);
                    }

                    if ui.button("Close").clicked() {
                        app.show_template_editor = false;
                        clear_editor_state(app);
                    }
                });
            });
        });

    // Handle window close button (X) or clicking outside
    if !open || clicked_outside {
        app.show_template_editor = false;
        clear_editor_state(app);
    }
}

fn handle_save(app: &mut App) {
    // Validation is done in create_template, but we check placeholder here for immediate feedback
    if !app.editor_template_input.contains("${description}") {
        app.editor_error = Some("Template must contain ${description} placeholder".to_string());
        return;
    }

    // Create template using core function
    match asset_tap_core::templates::create_template(
        &app.editor_name_input,
        &app.editor_description_input,
        &app.editor_template_input,
        None, // category
    ) {
        Ok(template_id) => {
            // Refresh available templates
            app.available_templates = asset_tap_core::templates::list_templates();

            // Select the newly created template
            app.template = Some(template_id.clone());

            // Show success toast
            app.toasts.push(crate::app::Toast::success(format!(
                "Template '{}' saved successfully",
                template_id
            )));

            // Close editor
            app.show_template_editor = false;
            clear_editor_state(app);
        }
        Err(e) => {
            app.editor_error = Some(e);
        }
    }
}

fn clear_editor_state(app: &mut App) {
    app.editing_template = None;
    app.editor_name_input.clear();
    app.editor_description_input.clear();
    app.editor_template_input.clear();
    app.editor_error = None;
    app.editor_mode = TemplateEditorMode::default();
}
