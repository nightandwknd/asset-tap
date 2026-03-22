//! Image approval panel for user review before 3D generation.

use crate::app::App;
use crate::icons;
use asset_tap_core::types::ApprovalData;
use eframe::egui;

/// Actions that can be requested from the approval panel.
#[derive(Debug)]
pub enum ApprovalAction {
    /// User approved the image - continue pipeline.
    Approve,
    /// User rejected the image - cancel and allow retry.
    Reject,
    /// User wants to regenerate with same prompt.
    Regenerate,
}

/// Render the regenerating loading state.
///
/// Shown while the pipeline is regenerating the image in-place.
pub fn render_regenerating(ui: &mut egui::Ui) {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.colored_label(egui::Color32::from_rgb(100, 200, 255), icons::ROTATE);
        ui.heading("Regenerating Image...");
    });
    ui.separator();

    ui.add_space(40.0);
    ui.vertical_centered(|ui| {
        ui.spinner();
        ui.add_space(12.0);
        ui.label("Generating a new image with the same prompt...");
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("This may take a moment")
                .small()
                .color(egui::Color32::GRAY),
        );
    });
    ui.add_space(40.0);

    ui.ctx().request_repaint();
}

/// Render the image approval panel.
///
/// Shows the generated image and approval controls.
pub fn render(
    ui: &mut egui::Ui,
    app: &mut App,
    approval_data: &ApprovalData,
) -> Option<ApprovalAction> {
    let mut action = None;
    let mut texture_loading = false;

    // Header
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.colored_label(egui::Color32::from_rgb(100, 200, 255), icons::CHECK_CIRCLE);
        ui.heading("Review Generated Image");
    });
    ui.separator();

    // Info panel
    egui::Frame::none()
        .fill(egui::Color32::from_rgb(40, 50, 60))
        .rounding(6.0)
        .inner_margin(egui::Margin::same(12.0))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(
                    "Please review the generated image before proceeding to 3D model generation.",
                )
                .color(egui::Color32::WHITE),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(format!("Prompt: \"{}\"", approval_data.prompt))
                    .small()
                    .color(egui::Color32::LIGHT_GRAY),
            );
            ui.label(
                egui::RichText::new(format!("Model: {}", approval_data.model))
                    .small()
                    .color(egui::Color32::LIGHT_GRAY),
            );
        });

    ui.add_space(12.0);

    // Load full-resolution approval image (not thumbnail) for quality review.
    // Lazy-load on first frame, then reuse the cached texture handle.
    let image_path = approval_data.image_path.clone();
    let needs_load = match &app.approval_texture {
        Some((cached_path, _)) => *cached_path != image_path,
        None => true,
    };
    if needs_load && image_path.exists() {
        if let Ok(img) = image::open(&image_path) {
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            let color_image =
                egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], rgba.as_raw());
            let texture =
                ui.ctx()
                    .load_texture("approval_image", color_image, egui::TextureOptions::LINEAR);
            app.approval_texture = Some((image_path.clone(), texture));
        }
    }

    // Image preview in scrollable area
    egui::ScrollArea::vertical()
        .max_height(500.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            if let Some((_, ref texture)) = app.approval_texture {
                let available_width = ui.available_width();
                let max_height = 450.0;

                let size = texture.size();
                let aspect_ratio = size[0] as f32 / size[1] as f32;
                let mut display_width = available_width.min(600.0);
                let mut display_height = display_width / aspect_ratio;

                // If height exceeds max, clamp and recalculate width to preserve aspect ratio
                if display_height > max_height {
                    display_height = max_height;
                    display_width = max_height * aspect_ratio;
                }

                ui.vertical_centered(|ui| {
                    ui.image(egui::load::SizedTexture::new(
                        texture.id(),
                        egui::vec2(display_width, display_height),
                    ));
                });
            } else {
                // Texture not loaded yet - mark for repaint
                texture_loading = true;

                // Check if file exists but hasn't loaded yet
                let file_exists = approval_data.image_path.exists();

                // Reserve space and show loading indicator or error
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), 200.0),
                    egui::Layout::centered_and_justified(egui::Direction::TopDown),
                    |ui| {
                        ui.vertical_centered(|ui| {
                            if file_exists {
                                ui.spinner();
                                ui.add_space(8.0);
                                ui.label("Loading image...");
                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new(format!(
                                        "Path: {}",
                                        approval_data.image_path.display()
                                    ))
                                    .small()
                                    .color(egui::Color32::GRAY),
                                );
                            } else {
                                ui.colored_label(
                                    egui::Color32::from_rgb(255, 180, 100),
                                    format!("{} Image file not found", icons::WARNING),
                                );
                                ui.add_space(8.0);
                                ui.label(
                                    egui::RichText::new(
                                        "The image may still be downloading or processing",
                                    )
                                    .small()
                                    .color(egui::Color32::GRAY),
                                );
                            }
                        });
                    },
                );
            }
        });

    ui.add_space(16.0);

    // Action buttons (manually centered like about modal)
    ui.horizontal(|ui| {
        let button_labels = [
            format!("{} Approve & Continue to 3D", icons::CHECK_CIRCLE),
            format!("{} Reject & Cancel", icons::CIRCLE_XMARK),
            format!("{} Regenerate Image", icons::ROTATE),
        ];
        let min_widths = [200.0_f32, 180.0, 180.0];
        let spacing = 12.0;

        // Measure actual button widths
        let mut total_width = 0.0;
        for (label, min_w) in button_labels.iter().zip(min_widths.iter()) {
            let text_width = ui.fonts(|f| {
                f.layout_no_wrap(label.clone(), egui::FontId::default(), egui::Color32::WHITE)
                    .size()
                    .x
            });
            let button_padding = ui.spacing().button_padding.x * 2.0;
            total_width += text_width.max(*min_w - button_padding) + button_padding;
        }
        total_width += spacing * 2.0 + ui.spacing().item_spacing.x * 2.0;

        let left_padding = ((ui.available_width() - total_width) / 2.0).max(0.0);
        ui.add_space(left_padding);

        if ui
            .add(
                egui::Button::new(
                    egui::RichText::new(&button_labels[0]).color(egui::Color32::WHITE),
                )
                .fill(egui::Color32::from_rgb(50, 150, 50))
                .min_size(egui::vec2(min_widths[0], 40.0)),
            )
            .clicked()
        {
            action = Some(ApprovalAction::Approve);
        }

        ui.add_space(spacing);

        if ui
            .add(
                egui::Button::new(
                    egui::RichText::new(&button_labels[1]).color(egui::Color32::WHITE),
                )
                .fill(egui::Color32::from_rgb(150, 50, 50))
                .min_size(egui::vec2(min_widths[1], 40.0)),
            )
            .clicked()
        {
            action = Some(ApprovalAction::Reject);
        }

        ui.add_space(spacing);

        if ui
            .add(
                egui::Button::new(
                    egui::RichText::new(&button_labels[2]).color(egui::Color32::WHITE),
                )
                .fill(egui::Color32::from_rgb(80, 100, 150))
                .min_size(egui::vec2(min_widths[2], 40.0)),
            )
            .clicked()
        {
            action = Some(ApprovalAction::Regenerate);
        }
    });

    ui.add_space(8.0);

    // Request repaint if texture is still loading
    if texture_loading {
        ui.ctx().request_repaint();
    }

    action
}
