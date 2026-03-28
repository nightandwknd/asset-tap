//! Progress display panel.

use crate::app::{App, RecoveryInfo};
use crate::icons;
use crate::style::RichTextExt;
use asset_tap_core::format_progress;
use asset_tap_core::progress_fmt::DisplayLevel;
use asset_tap_core::types::{Progress, Stage};
use eframe::egui;

/// Actions that can be requested from the progress panel.
#[derive(Debug)]
pub enum ProgressAction {
    /// User wants to retry with the saved image.
    RetryWithImage(std::path::PathBuf),
}

/// Render the progress panel.
pub fn render(app: &mut App, ui: &mut egui::Ui) {
    ui.add_space(4.0);

    let mut clear_logs = false;
    let mut cancel_clicked = false;
    ui.horizontal(|ui| {
        ui.heading("Progress");

        let state = app.state.lock().unwrap();
        let is_running = state.running;
        let has_progress = !state.progress.is_empty();
        drop(state);

        if is_running {
            ui.spinner();
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Cancel button (when running)
            if is_running
                && ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(format!("{} Cancel", icons::CIRCLE_XMARK))
                                .size(12.0)
                                .color(egui::Color32::WHITE),
                        )
                        .fill(egui::Color32::from_rgb(150, 50, 50)),
                    )
                    .clicked()
            {
                cancel_clicked = true;
            }

            // Clear logs button (only when not running and has content)
            if has_progress
                && !is_running
                && ui
                    .add(
                        egui::Button::new(egui::RichText::new(icons::PROHIBIT).size(14.0))
                            .frame(false),
                    )
                    .on_hover_text("Clear logs")
                    .clicked()
            {
                clear_logs = true;
            }
        });
    });

    if cancel_clicked {
        app.cancel_pipeline();
    }

    if clear_logs {
        let mut state = app.state.lock().unwrap();
        state.progress.clear();
        state.error = None;
        state.recovery_info = None;
    }

    ui.separator();

    // Check for recovery action outside of scroll area
    let mut action: Option<ProgressAction> = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            let state = app.state.lock().unwrap();

            if state.progress.is_empty() && !state.running && state.error.is_none() {
                ui.label(
                    egui::RichText::new("Ready to generate")
                        .secondary()
                        .italics(),
                );
            } else {
                let is_running = state.running;
                // Show progress items
                for progress in &state.progress {
                    render_progress_item(ui, progress, is_running);
                }

                // Show current stage indicator
                if let Some(stage) = state.current_stage {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(format!("{} in progress...", stage));
                    });
                }
            }

            // Show error and recovery at the bottom (after progress items)
            if let Some(ref error) = state.error {
                ui.add_space(8.0);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(format!("{} Error: {}", icons::CIRCLE_XMARK, error))
                            .color(egui::Color32::RED),
                    )
                    .wrap(),
                );

                // "View logs" link
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    if ui
                        .link(
                            egui::RichText::new(format!("{} View logs", icons::TERMINAL))
                                .small()
                                .color(egui::Color32::from_rgb(150, 150, 200)),
                        )
                        .clicked()
                    {
                        let logs_path = asset_tap_core::error_log::logs_dir_path();
                        std::fs::create_dir_all(&logs_path).ok();
                        crate::app::open_with_system(&logs_path, None);
                    }
                });

                // Show recovery option if available
                if let Some(ref recovery) = state.recovery_info {
                    ui.add_space(8.0);
                    action = render_recovery_panel(ui, recovery);
                }
            }
        });

    // Handle recovery action
    if let Some(ProgressAction::RetryWithImage(path)) = action {
        // Set the existing image and clear the error/recovery state
        // Keep progress logs so user can see the full history of events
        app.existing_image = Some(path.to_string_lossy().to_string());

        {
            let mut state = app.state.lock().unwrap();
            state.error = None;
            state.recovery_info = None;
            state.preserve_progress = true;
        }

        // Show feedback to user
        app.add_toast(crate::app::Toast::info(
            "Image loaded. Click Generate to continue.",
        ));
    }
}

/// Render the recovery panel for failed generations.
/// Returns an action if user clicks a recovery button.
fn render_recovery_panel(ui: &mut egui::Ui, recovery: &RecoveryInfo) -> Option<ProgressAction> {
    let mut action = None;

    egui::Frame::new()
        .fill(egui::Color32::from_rgb(50, 70, 50))
        .corner_radius(6)
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.colored_label(egui::Color32::from_rgb(100, 220, 100), icons::CIRCLE_CHECK);
                ui.label(
                    egui::RichText::new(&recovery.recovery_message).color(egui::Color32::WHITE),
                );
            });

            ui.add_space(8.0);

            ui.horizontal(|ui| {
                if ui
                    .add(egui::Button::new(format!(
                        "{} {}",
                        icons::ROTATE,
                        recovery.button_label
                    )))
                    .clicked()
                {
                    action = Some(ProgressAction::RetryWithImage(recovery.image_path.clone()));
                }

                ui.add_space(8.0);

                // Show the image path
                ui.label(
                    egui::RichText::new(
                        recovery
                            .image_path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default(),
                    )
                    .small()
                    .secondary(),
                );
            });
        });

    action
}

fn render_progress_item(ui: &mut egui::Ui, progress: &Progress, is_running: bool) {
    let display = format_progress(progress);

    // Get GUI-specific icon and color based on display level and progress type
    let (icon, color) = match progress {
        Progress::Started { stage, .. } => (stage_icon(stage), stage_color(stage)),
        Progress::Queued { .. } => (icons::CLOCK, egui::Color32::LIGHT_BLUE),
        Progress::Processing { .. } => (icons::ARROWS_ROTATE, egui::Color32::LIGHT_BLUE),
        Progress::Completed { .. } => (icons::CIRCLE_CHECK, egui::Color32::GREEN),
        Progress::Failed { .. } => (icons::CIRCLE_XMARK, egui::Color32::RED),
        Progress::Downloading { .. } => (icons::DOWNLOAD, egui::Color32::LIGHT_BLUE),
        Progress::Log { .. } => (icons::TERMINAL, egui::Color32::GRAY),
        Progress::Retrying { .. } => (icons::ARROWS_ROTATE, egui::Color32::YELLOW),
        Progress::AwaitingApproval { .. } => {
            (icons::CHECK_CIRCLE, egui::Color32::from_rgb(100, 200, 255))
        }
    };

    // Skip rendering Processing without a message (matches original behavior)
    if matches!(progress, Progress::Processing { message: None, .. }) {
        return;
    }

    ui.horizontal(|ui| {
        // Queued uses spinner only while pipeline is running; static icon otherwise
        if matches!(progress, Progress::Queued { .. }) && is_running {
            ui.spinner();
        } else {
            ui.colored_label(color, icon);
        }

        // Use shared message, with styling based on display level
        match display.level {
            DisplayLevel::Debug => {
                ui.label(egui::RichText::new(&display.message).small().secondary());
            }
            _ => {
                ui.label(&display.message);
            }
        }
    });
}

fn stage_icon(stage: &Stage) -> &'static str {
    match stage {
        Stage::ImageGeneration => icons::PALETTE,
        Stage::Model3DGeneration => icons::CUBE,
        Stage::FbxConversion => icons::ROTATE,
        Stage::Download => icons::DOWNLOAD,
    }
}

fn stage_color(stage: &Stage) -> egui::Color32 {
    match stage {
        Stage::ImageGeneration => egui::Color32::from_rgb(255, 180, 100), // Orange
        Stage::Model3DGeneration => egui::Color32::from_rgb(150, 100, 255), // Purple
        Stage::FbxConversion => egui::Color32::from_rgb(100, 255, 200),   // Cyan
        Stage::Download => egui::Color32::from_rgb(100, 200, 255),        // Light blue
    }
}
