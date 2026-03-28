//! Preview panel for generated assets.

use super::path_to_file_uri;
use crate::app::{App, PreviewTab};
use crate::icons;
use crate::style::RichTextExt;
use crate::views::walkthrough::WalkthroughStep;
use asset_tap_core::constants::files::{archive, bundle as bundle_files};
use eframe::egui;
use std::path::Path;

/// Extract a date-relative path from an absolute path.
/// Looks for a parent directory matching YYYYMMDD_HHMMSS pattern and returns
/// the path from that directory onwards (e.g., "20251225_233124/model.glb").
fn date_relative_path(path: &Path) -> String {
    let components: Vec<_> = path.components().collect();
    for (i, component) in components.iter().enumerate() {
        if let std::path::Component::Normal(name) = component {
            let name_str = name.to_string_lossy();
            // Match YYYY-MM-DD_HHMMSS pattern (17 chars)
            if name_str.len() == 17
                && name_str.chars().nth(10) == Some('_')
                && name_str[11..].chars().all(|c| c.is_ascii_digit())
            {
                // Build path from this component onwards
                let remaining: std::path::PathBuf = components[i..].iter().collect();
                return remaining.display().to_string();
            }
        }
    }
    // Fallback to filename only
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// Render the preview panel.
pub fn render(app: &mut App, ui: &mut egui::Ui) {
    app.walkthrough
        .register_rect(WalkthroughStep::PreviewPanel, ui.max_rect());
    ui.add_space(2.0);
    // Tab bar
    ui.horizontal(|ui| {
        if ui
            .add(egui::Button::selectable(
                app.preview_tab == PreviewTab::Image,
                format!("{} Image", icons::IMAGE),
            ))
            .clicked()
        {
            app.preview_tab = PreviewTab::Image;
        }

        if ui
            .add(egui::Button::selectable(
                app.preview_tab == PreviewTab::Model3D,
                format!("{} 3D Model", icons::CUBE),
            ))
            .clicked()
        {
            app.preview_tab = PreviewTab::Model3D;
        }

        if ui
            .add(egui::Button::selectable(
                app.preview_tab == PreviewTab::Textures,
                format!("{} Textures", icons::PALETTE),
            ))
            .clicked()
        {
            app.preview_tab = PreviewTab::Textures;
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Context-aware "Show Folder" button (appears on left due to right-to-left layout)
            let folder_to_open = app.output.as_ref().and_then(|output| {
                let path = match app.preview_tab {
                    PreviewTab::Image => output.image_path.as_ref(),
                    PreviewTab::Model3D => output.final_model_path(),
                    PreviewTab::Textures => output.textures_dir.as_ref(),
                };
                path.map(|p| {
                    if p.is_dir() {
                        p.to_path_buf()
                    } else {
                        p.parent()
                            .map_or_else(|| p.to_path_buf(), |par| par.to_path_buf())
                    }
                })
            });
            if let Some(folder) = folder_to_open {
                if ui
                    .button(format!("{} Show Folder", icons::FOLDER_OPEN))
                    .clicked()
                {
                    crate::app::open_with_system(&folder, Some(&mut app.toasts));
                }
            }

            // Context-aware library browser button (appears on right)
            match app.preview_tab {
                PreviewTab::Image => {
                    if ui
                        .button(format!("{} Browse Images", icons::BOOK))
                        .clicked()
                    {
                        app.open_library_for_image_preview();
                    }
                }
                PreviewTab::Model3D => {
                    if ui
                        .button(format!("{} Browse Models", icons::BOOK))
                        .clicked()
                    {
                        app.open_library_for_model_preview();
                    }
                }
                PreviewTab::Textures => {
                    if ui
                        .button(format!("{} Browse Textures", icons::BOOK))
                        .clicked()
                    {
                        app.open_library_for_textures_preview();
                    }
                }
            }
        });
    });

    ui.add_space(3.0);
    ui.separator();

    // Preview content
    let available = ui.available_size();

    match app.preview_tab {
        PreviewTab::Image => render_image_preview(app, ui, available),
        PreviewTab::Model3D => render_model_preview(app, ui, available),
        PreviewTab::Textures => render_textures_preview(app, ui, available),
    }
}

fn render_image_preview(app: &mut App, ui: &mut egui::Ui, available: egui::Vec2) {
    let output = app.output.clone();
    if let Some(ref output) = output {
        if let Some(ref path) = output.image_path {
            // Header
            ui.add_space(4.0);
            ui.heading(format!("{} Image", icons::IMAGE));
            ui.add_space(8.0);

            ui.vertical_centered(|ui| {
                // Calculate max size for image - use most of available space
                // Leave minimal padding (20px sides, 80px bottom for button)
                let max_size = egui::vec2(
                    (available.x - 20.0).max(100.0),
                    (available.y - 80.0).max(100.0),
                );

                // Display the image using file:// URI
                // Disable default spinner so we can use consistent ui.spinner() style
                let uri = path_to_file_uri(path);
                let image = egui::Image::new(&uri)
                    .max_size(max_size)
                    .maintain_aspect_ratio(true)
                    .corner_radius(4)
                    .show_loading_spinner(false);

                // Check if image is ready by trying to load it
                let is_loaded = ui
                    .ctx()
                    .try_load_texture(
                        &uri,
                        egui::TextureOptions::default(),
                        egui::SizeHint::Scale(1.0.into()),
                    )
                    .map(|poll| matches!(poll, egui::load::TexturePoll::Ready { .. }))
                    .unwrap_or(false);

                if is_loaded {
                    ui.add(image);
                } else {
                    // Show loading placeholder with consistent spinner
                    let placeholder_size = egui::vec2(max_size.x.min(300.0), max_size.y.min(300.0));
                    ui.allocate_ui(placeholder_size, |ui| {
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgb(40, 40, 45))
                            .corner_radius(4)
                            .show(ui, |ui| {
                                ui.set_min_size(placeholder_size);
                                ui.centered_and_justified(|ui| {
                                    ui.spinner();
                                });
                            });
                    });
                    // Still add the image (hidden) to trigger loading
                    ui.add(image);
                }

                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    if ui
                        .button(format!("{} Open Image", icons::EXTERNAL_LINK))
                        .on_hover_text("Open with system default viewer")
                        .clicked()
                    {
                        crate::app::open_with_system(path, Some(&mut app.toasts));
                    }

                    if ui
                        .button(format!("{} Export Image", icons::DOWNLOAD))
                        .on_hover_text("Save a copy to a custom location")
                        .clicked()
                    {
                        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("png");
                        let filename = format!("image.{}", ext);
                        if let Some(dest) = rfd::FileDialog::new()
                            .set_file_name(&filename)
                            .add_filter("Image", &[ext])
                            .save_file()
                        {
                            match std::fs::copy(path, &dest) {
                                Ok(_) => app
                                    .toasts
                                    .push(crate::app::Toast::success("Image exported")),
                                Err(e) => app.toasts.push(crate::app::Toast::error(format!(
                                    "Export failed: {}",
                                    e
                                ))),
                            }
                        }
                    }

                    ui.label(
                        egui::RichText::new(date_relative_path(path))
                            .small()
                            .secondary(),
                    );
                });
            });
        } else if let Some(ref url) = output.image_url {
            ui.vertical_centered(|ui| {
                ui.add_space(20.0);
                ui.label("Image URL:");
                ui.hyperlink(url);
            });
        } else if output.output_dir.is_some() {
            // Bundle exists but no image
            render_centered_message(
                ui,
                available,
                "No associated image for this bundle",
                &["The image file may be missing or wasn't generated"],
                icons::IMAGE,
            );
        } else {
            render_empty_state(ui, "No image generated yet");
        }
    } else {
        render_empty_state(ui, "Generate an asset to preview the image");
    }
}

fn render_model_preview(app: &mut App, ui: &mut egui::Ui, available: egui::Vec2) {
    // Clone output to avoid holding an immutable borrow of app throughout the function
    let output = app.output.clone();

    if let Some(ref output) = output {
        if let Some(path) = output.final_model_path() {
            // Initialize three-d context first (requires glow context)
            if let Some(ref gl) = app.gl_context {
                let mut viewer = app.model_viewer.lock().unwrap();
                viewer.init_context(gl.clone());
            }

            // Start async loading if not already loaded or loading
            {
                let mut viewer = app.model_viewer.lock().unwrap();
                if viewer.loaded_path() != Some(path.as_path()) && !viewer.is_loading() {
                    viewer.start_async_load(path.clone());
                }

                // Poll for loading completion
                viewer.poll_async_load();

                // Save model info to state when loading completes
                if let Some(ref info) = viewer.model_info {
                    if app.app_state.model_info.as_ref().map(|i| i.file_size)
                        != Some(info.file_size)
                    {
                        app.app_state.model_info = Some(asset_tap_core::state::ModelInfo {
                            file_size: info.file_size,
                            format: info.format.clone(),
                            vertex_count: info.vertex_count,
                            triangle_count: info.triangle_count,
                        });
                        let _ = app.app_state.save();
                    }
                }
            }

            // Check loading state
            let (is_loading, has_error) = {
                let viewer = app.model_viewer.lock().unwrap();
                (viewer.is_loading(), viewer.error.is_some())
            };

            // Show spinner if loading
            if is_loading {
                render_model_loading(ui);
                ui.ctx().request_repaint(); // Keep repainting to poll for results
                return;
            }

            if has_error {
                // Fallback to info display if 3D rendering fails
                render_model_info_fallback(app, ui, path, output);
                return;
            }

            // Header with controls
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let model_type = format!("{} 3D Model", icons::CUBE);
                ui.heading(model_type);

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button(format!("{} Reset View", icons::ROTATE_LEFT))
                        .clicked()
                    {
                        let mut viewer = app.model_viewer.lock().unwrap();
                        viewer.reset_camera();
                    }

                    // Viewer toggles
                    {
                        let mut viewer = app.model_viewer.lock().unwrap();

                        // Axes toggle
                        let mut show_axes = viewer.show_axes;
                        if ui.checkbox(&mut show_axes, "Axes").changed() {
                            viewer.toggle_axes();
                        }

                        // Grid toggle
                        let mut show_grid = viewer.show_grid;
                        if ui.checkbox(&mut show_grid, "Grid").changed() {
                            viewer.toggle_grid();
                        }
                    }
                });
            });

            ui.add_space(4.0);

            // Model info bar
            render_model_info_bar(app, ui);

            ui.add_space(8.0);

            // Calculate available space for the 3D viewer
            // Account for the controls above (~60px) and buttons below (~80px)
            let viewer_available = ui.available_size();
            let preview_size = egui::vec2(
                (viewer_available.x - 20.0).max(200.0),
                (viewer_available.y - 80.0).max(200.0),
            );

            // Center the 3D viewer horizontally
            ui.vertical_centered(|ui| {
                // Allocate space for the viewer with drag/scroll interaction
                let (rect, response) =
                    ui.allocate_exact_size(preview_size, egui::Sense::click_and_drag());

                // Handle camera controls (Blender-style)
                let mut needs_repaint = false;
                {
                    let mut viewer = app.model_viewer.lock().unwrap();

                    let modifiers = ui.input(|i| i.modifiers);

                    // Scroll handling (Blender-style)
                    // Mouse scroll wheel = ZOOM (discrete steps)
                    // Trackpad two-finger = ORBIT by default, ZOOM with Ctrl/Cmd, PAN with Shift
                    if response.hovered() {
                        // Extract scroll deltas and whether it's a trackpad (line vs pixel units)
                        let (scroll_x, scroll_y, is_trackpad) = ui.input(|i| {
                            let mut total_x = 0.0;
                            let mut total_y = 0.0;
                            let mut trackpad = false;
                            for event in &i.events {
                                if let egui::Event::MouseWheel { delta, unit, .. } = event {
                                    total_x += delta.x;
                                    total_y += delta.y;
                                    // Line units = discrete scroll wheel, Point/Pixel = trackpad
                                    if *unit != egui::MouseWheelUnit::Line {
                                        trackpad = true;
                                    }
                                }
                            }
                            (total_x, total_y, trackpad)
                        });

                        if scroll_x != 0.0 || scroll_y != 0.0 {
                            if is_trackpad {
                                // Trackpad: Ctrl/Cmd + scroll = ZOOM
                                if modifiers.ctrl || modifiers.command {
                                    viewer.camera_state.zoom(scroll_y * 0.01);
                                    viewer.mark_dirty();
                                    needs_repaint = true;
                                }
                                // Trackpad: Shift + scroll = PAN
                                else if modifiers.shift {
                                    viewer.camera_state.pan(-scroll_x, scroll_y);
                                    viewer.mark_dirty();
                                    needs_repaint = true;
                                }
                                // Trackpad: no modifiers = ORBIT
                                else {
                                    viewer.camera_state.rotate(scroll_x, scroll_y);
                                    viewer.mark_dirty();
                                    needs_repaint = true;
                                }
                            } else {
                                // Mouse scroll wheel: always ZOOM (Blender convention)
                                viewer.camera_state.zoom(scroll_y);
                                viewer.mark_dirty();
                                needs_repaint = true;
                            }
                        }
                    }

                    // Mouse drag controls (Blender-style)
                    if response.dragged() {
                        let delta = response.drag_delta();
                        // Shift + middle-mouse drag = Pan
                        if response.dragged_by(egui::PointerButton::Middle) && modifiers.shift {
                            viewer.camera_state.pan(delta.x, -delta.y);
                            viewer.mark_dirty();
                            needs_repaint = true;
                        }
                        // Middle-mouse drag = Orbit
                        else if response.dragged_by(egui::PointerButton::Middle) {
                            viewer.camera_state.rotate(delta.x, delta.y);
                            viewer.mark_dirty();
                            needs_repaint = true;
                        }
                        // Shift + left drag = Pan (convenience for trackpad users)
                        else if modifiers.shift {
                            viewer.camera_state.pan(delta.x, -delta.y);
                            viewer.mark_dirty();
                            needs_repaint = true;
                        }
                        // Left drag = Orbit
                        else {
                            viewer.camera_state.rotate(delta.x, delta.y);
                            viewer.mark_dirty();
                            needs_repaint = true;
                        }
                    }

                    // Pinch to zoom (trackpad pinch gesture)
                    if response.hovered() {
                        let zoom_delta = ui.input(|i| i.zoom_delta());
                        if zoom_delta != 1.0 {
                            // zoom_delta > 1.0 means spread/pinch out (zoom in)
                            // zoom_delta < 1.0 means pinch together (zoom out)
                            let zoom_factor = (zoom_delta - 1.0) * 2.0;
                            viewer.camera_state.zoom(zoom_factor);
                            viewer.mark_dirty();
                            needs_repaint = true;
                        }
                    }
                }

                // Render the 3D model via PaintCallback (direct GPU blit, no readback)
                {
                    let has_model = {
                        let viewer = app.model_viewer.lock().unwrap();
                        viewer.has_model()
                    };

                    if has_model && app.gl_context.is_some() {
                        // Draw dark background behind the 3D viewport
                        let painter = ui.painter_at(rect);
                        painter.rect_filled(rect, 4, egui::Color32::from_rgb(30, 30, 36));

                        // Add PaintCallback — three-d renders directly into
                        // egui's framebuffer at the correct viewport offset
                        let callback = crate::viewer::model::ModelViewer::paint_callback(
                            &app.model_viewer,
                            rect,
                        );
                        ui.painter().add(callback);
                    } else if app.gl_context.is_none() {
                        let painter = ui.painter_at(rect);
                        painter.rect_filled(rect, 8, egui::Color32::from_rgb(30, 30, 35));
                        painter.text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "3D rendering not available",
                            egui::FontId::proportional(16.0),
                            egui::Color32::GRAY,
                        );
                    }
                }

                // Overlay instructions
                let painter = ui.painter_at(rect);
                let text_pos = rect.left_bottom() + egui::vec2(10.0, -10.0);
                painter.text(
                    text_pos,
                    egui::Align2::LEFT_BOTTOM,
                    if cfg!(target_os = "linux") {
                        "Drag to rotate • Shift+Drag to pan • Ctrl+Scroll to zoom"
                    } else if cfg!(target_os = "macos") {
                        "Drag to rotate • Shift+Drag to pan • Ctrl+Scroll or Pinch to zoom"
                    } else {
                        "Drag to rotate • Shift+Drag to pan • Scroll or Pinch to zoom"
                    },
                    egui::FontId::proportional(12.0),
                    egui::Color32::from_white_alpha(128),
                );

                if needs_repaint {
                    ui.ctx().request_repaint();
                }
            });

            ui.add_space(12.0);

            // Action buttons with path
            render_model_action_buttons(app, ui, path, output);
        } else if output.output_dir.is_some() {
            // Bundle exists but no model
            render_centered_message(
                ui,
                available,
                "No associated 3D model for this bundle",
                &["The model file may be missing or wasn't generated"],
                icons::CUBE,
            );
        } else {
            render_empty_state(ui, "No 3D model generated yet");
        }
    } else {
        render_empty_state(ui, "Generate an asset to preview the model");
    }
}

/// Fallback display when 3D rendering isn't available.
fn render_model_info_fallback(
    app: &mut App,
    ui: &mut egui::Ui,
    path: &std::path::Path,
    output: &asset_tap_core::types::PipelineOutput,
) {
    // Header (same as successful view)
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let model_type = format!("{} 3D Model", icons::CUBE);
        ui.heading(model_type);
    });

    // Model info bar (same as successful view)
    render_model_info_bar(app, ui);

    ui.add_space(8.0);

    // Error placeholder instead of 3D viewer
    let available = ui.available_size();
    let placeholder_size = egui::vec2(
        (available.x - 20.0).max(200.0),
        (available.y - 80.0).max(200.0),
    );

    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(placeholder_size, egui::Sense::hover());

        // Draw background
        ui.painter()
            .rect_filled(rect, 8, egui::Color32::from_rgb(30, 30, 35));

        // Center content vertically and horizontally within the rect
        let content_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        let center = rect.center();

        // Get error message
        let error_msg = {
            let viewer = app.model_viewer.lock().unwrap();
            viewer.error.clone()
        };

        // Draw centered content
        let painter = content_ui.painter();

        // Icon
        painter.text(
            center - egui::vec2(0.0, 40.0),
            egui::Align2::CENTER_CENTER,
            icons::CUBE,
            egui::FontId::proportional(48.0),
            egui::Color32::from_white_alpha(100),
        );

        // Error message - display in a frame with proper text wrapping
        if let Some(ref error) = error_msg {
            let error_rect =
                egui::Rect::from_center_size(center, egui::vec2(available.x.min(600.0), 200.0));

            let mut child_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(error_rect)
                    .layout(egui::Layout::top_down(egui::Align::Center)),
            );

            egui::Frame::new()
                .fill(egui::Color32::from_rgba_premultiplied(40, 40, 20, 200))
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::same(16))
                .show(&mut child_ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("3D Preview Error")
                                .color(egui::Color32::YELLOW)
                                .strong()
                                .size(16.0),
                        );
                        ui.add_space(8.0);

                        // Split error message into lines and display each
                        for line in error.lines() {
                            let trimmed = line.trim();
                            if !trimmed.is_empty() {
                                ui.label(
                                    egui::RichText::new(trimmed)
                                        .color(egui::Color32::from_white_alpha(220))
                                        .size(13.0),
                                );
                            }
                        }

                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new("Open in external viewer to see the model")
                                .color(egui::Color32::from_white_alpha(160))
                                .italics()
                                .size(12.0),
                        );
                    });
                });
        } else {
            // Help text when there's no error
            painter.text(
                center,
                egui::Align2::CENTER_CENTER,
                "Open in external viewer to see the model",
                egui::FontId::proportional(13.0),
                egui::Color32::from_white_alpha(128),
            );
        }
    });

    ui.add_space(12.0);

    // Action buttons with path (same as successful view)
    render_model_action_buttons(app, ui, path, output);
}

/// Render the model info bar (format, size, vertex count, triangle count).
fn render_model_info_bar(app: &mut App, ui: &mut egui::Ui) {
    let viewer = app.model_viewer.lock().unwrap();
    if let Some(ref info) = viewer.model_info {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&info.format).secondary());
            ui.label("•");
            ui.label(egui::RichText::new(info.formatted_size()).secondary());
            ui.label("•");
            ui.label(egui::RichText::new(format!("{} verts", info.vertex_count)).secondary());
            ui.label("•");
            ui.label(egui::RichText::new(format!("{} tris", info.triangle_count)).secondary());
        });
    }
}

/// Render the model action buttons (Open GLB, Open FBX, Export GLB) with path display.
fn render_model_action_buttons(
    app: &mut App,
    ui: &mut egui::Ui,
    path: &std::path::Path,
    output: &asset_tap_core::types::PipelineOutput,
) {
    // Clone paths before entering closure to avoid borrow conflicts
    let fbx_path = output.fbx_path.clone();
    let glb_path = output.model_path.clone();
    let path_display = date_relative_path(path);

    ui.horizontal(|ui| {
        if ui
            .button(format!("{} Open GLB", icons::EXTERNAL_LINK))
            .on_hover_text("Open with system default viewer")
            .clicked()
        {
            crate::app::open_with_system(path, Some(&mut app.toasts));
        }

        if let Some(ref fbx) = fbx_path {
            let blender_available = app.blender_available;

            let button = egui::Button::new(format!("{} Open FBX", icons::FILE));
            let mut response = ui.add_enabled(blender_available, button);

            if blender_available {
                response = response.on_hover_text("Open with Blender");
            } else {
                response = response.on_disabled_hover_text(
                    "Blender not found. Please install Blender to open FBX files.",
                );
            }

            if response.clicked() {
                app.open_fbx_in_blender(fbx);
            }
        } else if glb_path.is_some() {
            // No FBX yet — offer to convert the existing GLB
            let blender_available = app.blender_available;
            let has_custom_blender = app
                .settings
                .blender_path
                .as_ref()
                .is_some_and(|p| !p.is_empty());
            let converting = app.pending_fbx_conversion.is_some();
            let can_convert = (blender_available || has_custom_blender) && !converting;

            let label = if converting {
                format!("{} Converting...", icons::FILE)
            } else {
                format!("{} Convert to FBX", icons::FILE)
            };
            let button = egui::Button::new(label);
            let mut response = ui.add_enabled(can_convert, button);

            if converting {
                response = response.on_disabled_hover_text("FBX conversion in progress...");
            } else if blender_available || has_custom_blender {
                response = response.on_hover_text("Convert GLB to FBX using Blender");
            } else {
                response = response.on_disabled_hover_text(
                    "Blender not found. Install Blender to enable FBX conversion.",
                );
            }

            if response.clicked() && app.pending_fbx_conversion.is_none() {
                if let Some(ref glb) = glb_path {
                    app.start_fbx_conversion(glb.clone());
                }
            }
        }

        if let Some(ref glb) = glb_path {
            if glb != path && ui.button(format!("{} Open GLB", icons::FILE)).clicked() {
                crate::app::open_with_system(glb, Some(&mut app.toasts));
            }
        }

        if ui
            .button(format!("{} Export GLB", icons::DOWNLOAD))
            .on_hover_text("Save a copy to a custom location")
            .clicked()
        {
            if let Some(dest) = rfd::FileDialog::new()
                .set_file_name(bundle_files::MODEL_GLB)
                .add_filter("GLB", &["glb"])
                .save_file()
            {
                match std::fs::copy(path, &dest) {
                    Ok(_) => app.toasts.push(crate::app::Toast::success("GLB exported")),
                    Err(e) => app
                        .toasts
                        .push(crate::app::Toast::error(format!("Export failed: {}", e))),
                }
            }
        }

        ui.label(egui::RichText::new(path_display).small().secondary());
    });
}

fn render_textures_preview(app: &mut App, ui: &mut egui::Ui, available: egui::Vec2) {
    // Get textures_dir from output
    let textures_dir = app.output.as_ref().and_then(|o| o.textures_dir.clone());

    if let Some(ref dir) = textures_dir {
        // Update texture cache with current directory
        app.texture_cache.set_directory(Some(dir));

        // Process any loaded thumbnails from background threads
        if app.texture_cache.process_loaded(ui.ctx()) {
            ui.ctx().request_repaint();
        }

        // Header
        ui.add_space(4.0);
        ui.heading(format!("{} Textures", icons::PALETTE));
        ui.add_space(8.0);

        // Collect texture paths (typically just 2: Image_0.png and Image_1.png)
        let mut texture_paths: Vec<_> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let is_texture = path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| {
                        matches!(ext.to_lowercase().as_str(), "png" | "jpg" | "jpeg")
                    });
                if is_texture {
                    Some(path)
                } else {
                    None
                }
            })
            .collect();
        texture_paths.sort();

        // Calculate size for 2 images side by side
        // Use more of the available space (only 20px padding total)
        let spacing = 20.0;
        let max_size = egui::vec2(
            ((available.x - spacing - 20.0) / 2.0).max(100.0),
            (available.y - 80.0).max(100.0),
        );

        ui.vertical_centered(|ui| {
            ui.horizontal(|ui| {
                for path in &texture_paths {
                    ui.vertical(|ui| {
                        // Display texture using the same pattern as image preview
                        if let Some(texture) = app.texture_cache.get_thumbnail(path) {
                            let response = ui.add(
                                egui::Image::from_texture(texture)
                                    .max_size(max_size)
                                    .maintain_aspect_ratio(true)
                                    .corner_radius(4)
                                    .sense(egui::Sense::click()),
                            );
                            if response.clicked() {
                                crate::app::open_with_system(path, Some(&mut app.toasts));
                            }
                        } else {
                            // Loading placeholder with spinner
                            let placeholder =
                                egui::vec2(max_size.x.min(200.0), max_size.y.min(200.0));
                            ui.allocate_ui(placeholder, |ui| {
                                egui::Frame::new()
                                    .fill(egui::Color32::from_rgb(40, 40, 45))
                                    .corner_radius(4)
                                    .show(ui, |ui| {
                                        ui.set_min_size(placeholder);
                                        ui.centered_and_justified(|ui| {
                                            ui.spinner();
                                        });
                                    });
                            });
                        }

                        // Filename
                        let name = path.file_name().unwrap_or_default().to_string_lossy();
                        ui.label(egui::RichText::new(name.as_ref()).small());
                    });

                    ui.add_space(spacing);
                }
            });
        });

        ui.add_space(10.0);

        ui.horizontal(|ui| {
            if ui
                .button(format!("{} Export Textures", icons::DOWNLOAD))
                .on_hover_text("Save textures as a zip archive")
                .clicked()
            {
                if let Some(dest) = rfd::FileDialog::new()
                    .set_file_name(archive::TEXTURES_ZIP)
                    .add_filter("ZIP Archive", &["zip"])
                    .save_file()
                {
                    match export_textures_zip(&texture_paths, &dest) {
                        Ok(count) => app.toasts.push(crate::app::Toast::success(format!(
                            "Exported {} textures",
                            count
                        ))),
                        Err(e) => app
                            .toasts
                            .push(crate::app::Toast::error(format!("Export failed: {}", e))),
                    }
                }
            }
        });

        // Request repaint while thumbnails are still loading
        if app.texture_cache.has_pending() {
            ui.ctx().request_repaint();
        }
    } else {
        // Clear texture cache when no directory
        app.texture_cache.set_directory(None);

        if let Some(ref output) = app.output {
            if output.output_dir.is_some() {
                // Bundle exists but no textures - explain why
                render_centered_message(
                    ui,
                    available,
                    "No textures for this bundle",
                    &[
                        "💡 Textures are only extracted when FBX export is enabled",
                        "Enable 'Export FBX' in the sidebar before generating",
                    ],
                    icons::WARNING,
                );
            } else {
                render_empty_state(
                    ui,
                    "No textures extracted (FBX conversion may have been skipped)",
                );
            }
        } else {
            render_empty_state(ui, "Generate an asset to preview textures");
        }
    }
}

/// Render a centered warning/info message with consistent styling.
/// Used across preview tabs for warnings, errors, and informational messages.
fn render_centered_message(
    ui: &mut egui::Ui,
    available: egui::Vec2,
    title: &str,
    lines: &[&str],
    icon: &str,
) {
    let placeholder_size = egui::vec2(
        (available.x - 20.0).max(200.0),
        (available.y - 80.0).max(200.0),
    );

    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(placeholder_size, egui::Sense::hover());

        // Draw background
        ui.painter()
            .rect_filled(rect, 8, egui::Color32::from_rgb(30, 30, 35));

        // Center content within the rect
        let center = rect.center();
        let message_rect =
            egui::Rect::from_center_size(center, egui::vec2(available.x.min(600.0), 200.0));

        let mut child_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(message_rect)
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );

        egui::Frame::new()
            .fill(egui::Color32::from_rgba_premultiplied(40, 40, 20, 200))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::same(16))
            .show(&mut child_ui, |ui| {
                ui.vertical_centered(|ui| {
                    // Title with icon
                    ui.label(
                        egui::RichText::new(format!("{} {}", icon, title))
                            .color(egui::Color32::YELLOW)
                            .strong()
                            .size(16.0),
                    );
                    ui.add_space(8.0);

                    // Message lines
                    for (i, line) in lines.iter().enumerate() {
                        let alpha = if i == lines.len() - 1 { 160 } else { 220 };
                        let is_last = i == lines.len() - 1;

                        let mut text = egui::RichText::new(*line)
                            .color(egui::Color32::from_white_alpha(alpha))
                            .size(if is_last { 12.0 } else { 13.0 });

                        if is_last {
                            text = text.italics();
                        }

                        ui.label(text);

                        if i < lines.len() - 1 {
                            ui.add_space(4.0);
                        }
                    }
                });
            });
    });
}

fn render_empty_state(ui: &mut egui::Ui, message: &str) {
    ui.centered_and_justified(|ui| {
        ui.label(
            egui::RichText::new(message)
                .size(16.0)
                .secondary()
                .italics(),
        );
    });
}

/// Render a loading indicator for the model viewer.
fn render_model_loading(ui: &mut egui::Ui) {
    ui.add_space(4.0);
    ui.heading(format!("{} 3D Model", icons::CUBE));
    ui.add_space(20.0);

    // Get available space for the loading indicator
    let available = ui.available_size();
    let placeholder_size = egui::vec2(
        (available.x - 20.0).max(200.0),
        (available.y - 80.0).max(200.0),
    );

    // Center horizontally
    ui.vertical_centered(|ui| {
        // Allocate the full size for the loading area
        let (rect, _) = ui.allocate_exact_size(placeholder_size, egui::Sense::hover());

        // Draw the background frame
        ui.painter()
            .rect_filled(rect, 8, egui::Color32::from_rgb(40, 40, 45));

        // Calculate center position for spinner and text
        let center = rect.center();

        // Draw spinner at center
        ui.put(
            egui::Rect::from_center_size(center + egui::vec2(0.0, -15.0), egui::vec2(20.0, 20.0)),
            egui::Spinner::new(),
        );

        // Draw text below spinner
        let text = "Loading model...";
        let text_galley = ui.painter().layout_no_wrap(
            text.to_string(),
            egui::FontId::proportional(14.0),
            egui::Color32::from_white_alpha(180),
        );
        let text_pos = egui::pos2(center.x - text_galley.size().x / 2.0, center.y + 10.0);
        ui.painter()
            .galley(text_pos, text_galley, egui::Color32::from_white_alpha(180));
    });
}

/// Create a zip archive from texture files.
fn export_textures_zip(
    texture_paths: &[std::path::PathBuf],
    dest: &std::path::Path,
) -> Result<usize, String> {
    let file = std::fs::File::create(dest).map_err(|e| format!("Failed to create zip: {}", e))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let mut count = 0;
    for src in texture_paths {
        let name = src
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let data = std::fs::read(src).map_err(|e| format!("Failed to read {}: {}", name, e))?;
        zip.start_file(&name, options)
            .map_err(|e| format!("Failed to add {}: {}", name, e))?;
        std::io::Write::write_all(&mut zip, &data)
            .map_err(|e| format!("Failed to write {}: {}", name, e))?;
        count += 1;
    }

    zip.finish()
        .map_err(|e| format!("Failed to finalize zip: {}", e))?;
    Ok(count)
}
