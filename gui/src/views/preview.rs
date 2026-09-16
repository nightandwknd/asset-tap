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
            // Match YYYY-MM-DD_HHMMSS pattern (17 chars). Use `.get()` rather
            // than a byte-index slice so a non-ASCII name that happens to be 17
            // bytes can't panic on a char boundary.
            if name_str.len() == 17
                && name_str.chars().nth(10) == Some('_')
                && name_str
                    .get(11..)
                    .is_some_and(|s| s.chars().all(|c| c.is_ascii_digit()))
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
    ui.add_space(4.0);
    // Tab bar
    ui.horizontal(|ui| {
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
                app.preview_tab == PreviewTab::Image,
                format!("{} Image", icons::IMAGE),
            ))
            .clicked()
        {
            app.preview_tab = PreviewTab::Image;
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
            if let Some(folder) = folder_to_open
                && ui
                    .button(format!("{} Show Folder", icons::FOLDER_OPEN))
                    .clicked()
            {
                crate::app::open_with_system(&folder, Some(&mut app.toasts));
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

                let image_response = if is_loaded {
                    Some(ui.add(image.sense(egui::Sense::click())))
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
                    None
                };

                // Right-click context menu on the displayed image — offers the
                // "use this for a new generation" shortcut so the user doesn't
                // have to re-navigate via the sidebar's file picker. Only wired
                // when the image is actually loaded and visible to the user.
                let mut use_for_generation = false;
                if let Some(resp) = image_response {
                    resp.context_menu(|ui| {
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                        if ui.button("Use for generation").clicked() {
                            use_for_generation = true;
                            ui.close();
                        }
                    });
                }
                if use_for_generation {
                    app.queue_image_for_generation(path.to_string_lossy().into_owned());
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
                        .button(format!("{} Use for Generation", icons::MAGIC_WAND))
                        .on_hover_text("Load this image into the sidebar to skip text-to-image on the next generation")
                        .clicked()
                    {
                        app.queue_image_for_generation(path.to_string_lossy().into_owned());
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

/// Starting width of the Animation column. Wide enough for the clip rows and
/// the Rig button groups without wrapping.
const ANIMATE_PANEL_WIDTH: f32 = 288.0;
/// Below this the Rig button rows start wrapping again.
const MIN_ANIMATE_PANEL_WIDTH: f32 = 250.0;
/// Above this the column eats the viewer for no benefit.
const MAX_ANIMATE_PANEL_WIDTH: f32 = 460.0;

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
                if let Some(ref info) = viewer.model_info
                    && app.app_state.model_info.as_ref().map(|i| i.file_size)
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

                    {
                        let mut viewer = app.model_viewer.lock().unwrap();
                        let mut show_axes = viewer.show_axes;
                        if ui.checkbox(&mut show_axes, "Axes").changed() {
                            viewer.toggle_axes();
                        }
                        let mut show_grid = viewer.show_grid;
                        if ui.checkbox(&mut show_grid, "Grid").changed() {
                            viewer.toggle_grid();
                        }
                    }

                    ui.separator();
                    let placing = app.model_viewer.lock().unwrap().is_placing();
                    ui.add_enabled_ui(!placing, |ui| {
                        if ui
                            .button(format!("{} Animate", icons::PLAY))
                            .on_hover_text(if placing {
                                "Bind or Cancel the rig first"
                            } else if app.workbench_animate {
                                "Close the Animation panel"
                            } else {
                                "Open the Animation panel"
                            })
                            .clicked()
                        {
                            if app.workbench_animate {
                                app.close_animate_panel();
                            } else {
                                app.open_animate_panel();
                            }
                        }
                    });
                });
            });

            ui.add_space(4.0);

            // Model info bar
            render_model_info_bar(app, ui);

            ui.add_space(6.0);

            // The Animation panel is a right-hand column, not a bar above the
            // viewer. The clip list is tall, and stacking it would shrink the
            // 3D view by roughly its own height every time the panel opened.
            // `Panel::right` matches the sidebar and bundle-info panes, so the
            // drag handle behaves the way the rest of the app does.
            if app.workbench_animate {
                egui::Panel::right("animate_panel")
                    .resizable(true)
                    .default_size(ANIMATE_PANEL_WIDTH)
                    .min_size(MIN_ANIMATE_PANEL_WIDTH)
                    .max_size(MAX_ANIMATE_PANEL_WIDTH)
                    .show_inside(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("animate_panel")
                            .auto_shrink([false, false])
                            .show(ui, |ui| render_workbench_bar(app, ui));
                    });
            }

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
                let viewer_busy = app.workbench_busy();
                let (rect, response) =
                    ui.allocate_exact_size(preview_size, egui::Sense::click_and_drag());

                // Handle camera controls (Blender-style)
                let mut needs_repaint = false;
                if viewer_busy {
                    needs_repaint = true;
                }
                if !viewer_busy {
                    let mut viewer = app.model_viewer.lock().unwrap();
                    let modifiers = ui.input(|i| i.modifiers);
                    let place_drag = viewer.is_place_dragging();
                    let place_consumed = viewer.handle_place(rect, &response, modifiers.shift);
                    if place_consumed || place_drag || viewer.is_placing() {
                        needs_repaint = true;
                    }

                    let allow_orbit = !place_consumed && !place_drag;

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
                                else if allow_orbit && modifiers.shift {
                                    viewer.camera_state.pan(-scroll_x, scroll_y);
                                    viewer.mark_dirty();
                                    needs_repaint = true;
                                }
                                // Trackpad: no modifiers = ORBIT
                                else if allow_orbit {
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
                    if allow_orbit && response.dragged() {
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
                        render_marker_labels(app, ui, rect);
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

                if viewer_busy {
                    let painter = ui.painter_at(rect);
                    painter.rect_filled(rect, 4, egui::Color32::from_black_alpha(160));
                    painter.text(
                        rect.center() + egui::vec2(0.0, 22.0),
                        egui::Align2::CENTER_CENTER,
                        "Working…",
                        egui::FontId::proportional(16.0),
                        egui::Color32::from_white_alpha(220),
                    );
                    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                        ui.centered_and_justified(|ui| {
                            ui.spinner();
                        });
                    });
                } else {
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
                }

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

/// Optional Animation panel. Hidden until the user opens Animate.
/// Rig (pose + bind) then clip preview / bake live only inside this panel.
fn render_workbench_bar(app: &mut App, ui: &mut egui::Ui) {
    if !app.workbench_animate {
        return;
    }

    let busy = app.workbench_busy();
    let model_path = app
        .output
        .as_ref()
        .and_then(|o| o.final_model_path().map(|p| p.to_path_buf()));
    // Rigging uses the embedded canonical skeleton, so it needs no pack.
    // Only clip preview and Bake depend on an installed library.
    let has_clips = !app.clip_catalog.is_empty();
    let (
        has_clip,
        playing,
        show_bones,
        time,
        duration,
        placing,
        can_undo,
        can_redo,
        selected,
        fitted,
    ) = {
        let viewer = app.model_viewer.lock().unwrap();
        let (t, d) = viewer.play_time();
        (
            viewer.has_clip(),
            viewer.is_playing(),
            viewer.show_bones,
            t,
            d,
            viewer.is_placing(),
            viewer.can_undo_place(),
            viewer.can_redo_place(),
            viewer.selected_bone().map(str::to_string),
            viewer.is_fitted(),
        )
    };

    egui::Frame::group(ui.style())
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);

            if placing {
                let seeding = app.model_viewer.lock().unwrap().awaiting_seed();
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Rig").strong());
                    if busy {
                        ui.spinner();
                        ui.label(
                            egui::RichText::new(if seeding {
                                "Fitting skeleton\u{2026}"
                            } else {
                                "Working\u{2026}"
                            })
                            .small()
                            .weak(),
                        );
                    }
                });
                ui.add_space(6.0);

                // Grouped by what the button does, one purpose per row: the
                // wrapped single row overflowed a narrow column and split
                // Front/Side across lines.
                ui.add_enabled_ui(!busy, |ui| {
                    if ui
                        .add_sized([ui.available_width(), 24.0], egui::Button::new("Auto-fit"))
                        .on_hover_text("Guess the skeleton. Review, then Bind or Undo / Cancel.")
                        .clicked()
                    {
                        app.start_reseed();
                    }
                });

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(!busy && can_undo, |ui| {
                        if ui.button("Undo").clicked() {
                            app.model_viewer.lock().unwrap().undo_place();
                        }
                    });
                    ui.add_enabled_ui(!busy && can_redo, |ui| {
                        if ui.button("Redo").clicked() {
                            app.model_viewer.lock().unwrap().redo_place();
                        }
                    });
                });

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("View").small().weak());
                    ui.add_enabled_ui(!busy, |ui| {
                        if ui.button("Front").clicked() {
                            app.model_viewer.lock().unwrap().view_front();
                        }
                        if ui.button("Side").clicked() {
                            app.model_viewer.lock().unwrap().view_side();
                        }
                        ui.add_enabled_ui(selected.is_some(), |ui| {
                            if ui
                                .button("Frame")
                                .on_hover_text("Frame the selected joint")
                                .clicked()
                            {
                                app.model_viewer.lock().unwrap().frame_selected();
                            }
                        });
                    });
                });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(!busy && !seeding, |ui| {
                        if ui
                            .add_sized([84.0, 26.0], egui::Button::new("Bind"))
                            .on_hover_text(
                                "Skin the mesh to this skeleton. Clip preview unlocks after.",
                            )
                            .clicked()
                        {
                            app.commit_place();
                        }
                    });
                    ui.add_enabled_ui(!busy, |ui| {
                        if ui
                            .button("Cancel")
                            .on_hover_text("Discard this pose and leave")
                            .clicked()
                        {
                            app.model_viewer.lock().unwrap().exit_place();
                        }
                    });
                });

                if let Some(name) = &selected {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(panel_joint(name)).small());
                }

                ui.add_space(6.0);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(
                            "Park every joint on the body. Bind skins the mesh to this \
                             skeleton; a joint off the mesh is refused.",
                        )
                        .small()
                        .weak(),
                    )
                    .wrap(),
                );
                render_marker_legend(app, ui);
                return;
            }

            if !fitted {
                ui.label(egui::RichText::new("Rig").strong());
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.add_enabled_ui(!busy && model_path.is_some(), |ui| {
                        if ui
                            .button("Rig")
                            .on_hover_text("Pose the skeleton on the mesh, then Bind.")
                            .clicked()
                            && let Err(e) = app.model_viewer.lock().unwrap().enter_place()
                        {
                            app.toasts.push(crate::app::Toast::error(e));
                        }
                    });
                    if busy {
                        ui.spinner();
                    }
                });
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Clip preview unlocks after Bind.")
                        .small()
                        .weak(),
                );
                return;
            }

            ui.label(egui::RichText::new("Animation").strong());
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.add_enabled_ui(!busy && model_path.is_some(), |ui| {
                    if ui
                        .button("Rig")
                        .on_hover_text("Re-pose the skeleton and Bind again.")
                        .clicked()
                        && let Err(e) = app.model_viewer.lock().unwrap().enter_place()
                    {
                        app.toasts.push(crate::app::Toast::error(e));
                    }
                });
                ui.separator();
                let mut bones = show_bones;
                if ui.checkbox(&mut bones, "Bones").changed() {
                    app.workbench_show_bones = bones;
                    app.model_viewer.lock().unwrap().set_show_bones(bones);
                }
                if busy {
                    ui.spinner();
                }
            });

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Clips").small().weak());
                ui.add(
                    egui::TextEdit::singleline(&mut app.clip_filter)
                        .desired_width(140.0)
                        .hint_text("Search"),
                );
                if !app.clip_filter.is_empty() && ui.small_button("\u{2715}").clicked() {
                    app.clip_filter.clear();
                }
            });
            ui.add_space(2.0);
            render_clip_list(app, ui, fitted, busy);

            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.add_enabled_ui(!busy && model_path.is_some() && has_clips && fitted, |ui| {
                    let label = if playing { "Pause" } else { "Play" };
                    if ui.button(label).clicked() {
                        // Nothing loaded yet (a fresh Bind, or the panel just
                        // opened): load the selection rather than doing
                        // nothing, so Play always means play.
                        if has_clip {
                            app.model_viewer.lock().unwrap().toggle_playing();
                        } else {
                            let id = app.clip.clone();
                            app.preview_clip(&id);
                        }
                    }
                });
                if duration > 0.0 {
                    ui.label(format!("{time:.2} / {duration:.2}s"));
                    let mut scrub = time;
                    if ui
                        .add_sized(
                            [90.0, 18.0],
                            egui::Slider::new(&mut scrub, 0.0..=duration).show_value(false),
                        )
                        .changed()
                    {
                        app.model_viewer.lock().unwrap().seek(scrub);
                    }
                }
            });

            // Bake is declarative, so unticking removes. Say that here rather
            // than letting the button change meaning under the author.
            let checked = app.bake_set.len();
            let (adding, removing) = app.bake_delta();
            ui.add_space(4.0);
            if adding > 0 || removing > 0 {
                let mut parts = Vec::new();
                if adding > 0 {
                    parts.push(format!("+{adding}"));
                }
                if removing > 0 {
                    parts.push(format!("\u{2212}{removing}"));
                }
                ui.label(
                    egui::RichText::new(format!(
                        "{} in model \u{2192} {} after bake  ({})",
                        app.model_clips.len(),
                        checked,
                        parts.join(" ")
                    ))
                    .small()
                    .weak(),
                );
            } else if checked > 0 {
                ui.label(
                    egui::RichText::new(format!("{checked} in model \u{2014} up to date"))
                        .small()
                        .weak(),
                );
            }

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_enabled_ui(
                    !busy && model_path.is_some() && fitted && checked > 0,
                    |ui| {
                        let label = if checked == 1 {
                            "Bake 1 clip".to_string()
                        } else {
                            format!("Bake {checked} clips")
                        };
                        let button = ui.button(label).on_hover_text(
                            "Write the ticked clips into model.glb. The file ends up with \
                         exactly this set, so unticking one removes it.",
                        );
                        if button.clicked()
                            && let Some(path) = model_path.clone()
                        {
                            let clips = app.bake_clips();
                            app.start_bake(path, clips);
                        }
                    },
                );
                if checked == 0 && fitted {
                    ui.label(egui::RichText::new("Tick a clip to bake").small().weak());
                }
            });

            // Stripping every animation is a separate, destructive intent —
            // not a Bake with nothing ticked. It stays visible but disabled
            // when there is nothing to clear: hiding it entirely made the
            // capability indistinguishable from a bug.
            if fitted {
                let has_baked = !app.model_clips.is_empty();
                ui.add_space(2.0);
                ui.add_enabled_ui(!busy && has_baked, |ui| {
                    if ui
                        .small_button("Clear animation")
                        .on_hover_text(if has_baked {
                            "Remove every animation from model.glb, keeping the rig"
                        } else {
                            "Nothing to clear. This model has no baked animation"
                        })
                        .clicked()
                    {
                        app.pending_clear_animation = true;
                    }
                });
            }
        });
}

/// Same words as the legend: VRM `leftShoulder` is "Left clavicle".
fn panel_joint(name: &str) -> String {
    asset_tap_core::HumanBone::parse(name)
        .map(|b| b.panel_label())
        .unwrap_or_else(|| name.to_string())
}

/// Side labels over the Rig markers.
///
/// Color says which joint a marker is; this says which of a pair. Twins are
/// indistinguishable once the camera turns, and `DEF-thigh.L` was never
/// something an author should have to read off a tooltip.
fn render_marker_labels(app: &App, ui: &egui::Ui, rect: egui::Rect) {
    let labels = {
        let viewer = app.model_viewer.lock().unwrap();
        if !viewer.is_placing() {
            return;
        }
        viewer.marker_labels(rect)
    };
    let painter = ui.painter_at(rect);
    for label in labels {
        let size = if label.emphasized { 13.0 } else { 11.0 };
        // A dark disc keeps the glyph legible where a marker sits over a
        // light patch of the mesh.
        painter.circle_filled(
            label.screen,
            size * 0.72,
            egui::Color32::from_black_alpha(170),
        );
        painter.text(
            label.screen,
            egui::Align2::CENTER_CENTER,
            label.text,
            egui::FontId::proportional(size),
            label.color,
        );
    }
}

/// Swatch + name for each body part on screen, so a color means something.
fn render_marker_legend(app: &App, ui: &mut egui::Ui) {
    let legend = app.model_viewer.lock().unwrap().marker_legend();
    if legend.is_empty() {
        return;
    }
    ui.add_space(4.0);
    ui.label(egui::RichText::new("Legend").small().weak());
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (group, color) in legend {
            // Allocate swatch + name as one unit, or wrapping strands a label
            // on the next line away from the color it names.
            let text = egui::RichText::new(group.label()).small();
            let galley = ui.painter().layout_no_wrap(
                group.label().to_string(),
                egui::FontId::proportional(10.0),
                egui::Color32::PLACEHOLDER,
            );
            let width = galley.size().x + 16.0;
            ui.allocate_ui_with_layout(
                egui::vec2(width, 14.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(9.0, 9.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 2, color);
                    ui.label(text);
                },
            );
        }
    });
}

/// The clip catalog: tick to export, click to play.
///
/// Two selections live here and they are deliberately different — the ticked
/// set is what Bake writes, while the highlighted row is only what the viewer
/// is playing. Clicking never changes the export set, and ticking never
/// changes playback.
fn render_clip_list(app: &mut App, ui: &mut egui::Ui, fitted: bool, busy: bool) {
    if app.clip_catalog.is_empty() {
        ui.label(
            egui::RichText::new("No animation packs installed")
                .small()
                .color(egui::Color32::from_rgb(255, 180, 100)),
        );
        ui.label(
            egui::RichText::new("Rigging still works. Packs only add clips.")
                .small()
                .weak(),
        );
        ui.add_space(4.0);
        render_quaternius_pack_links(app, ui);
        ui.add_space(4.0);
        let downloading = app.clip_packs_downloading();
        if ui
            .add_enabled(
                !downloading,
                egui::Button::new(if downloading {
                    "Downloading packs…"
                } else {
                    "Download free packs"
                }),
            )
            .on_hover_text("Quaternius Standard (CC0), hash-verified from the latest release")
            .clicked()
        {
            app.request_clip_packs_download();
        }
        ui.add_space(4.0);
        render_install_pack_button(app, ui);
        return;
    }

    let filter = app.clip_filter.to_ascii_lowercase();
    let rows: Vec<(String, String, String, String)> = app
        .clip_catalog
        .iter()
        .filter(|c| {
            filter.is_empty()
                || c.name.to_ascii_lowercase().contains(&filter)
                || c.pack_name.to_ascii_lowercase().contains(&filter)
        })
        .map(|c| {
            (
                c.id.clone(),
                c.name.clone(),
                c.pack_id.clone(),
                c.pack_name.clone(),
            )
        })
        .collect();

    if rows.is_empty() {
        ui.label(
            egui::RichText::new("No clips match that filter")
                .small()
                .weak(),
        );
        return;
    }

    let mut toggled: Option<String> = None;
    let mut play: Option<String> = None;
    egui::Frame::group(ui.style())
        .inner_margin(egui::Margin::same(4))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .max_height(240.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut pack = String::new();
                    for (id, name, pack_id, pack_name) in &rows {
                        if *pack_id != pack {
                            if !pack.is_empty() {
                                ui.add_space(4.0);
                            }
                            ui.label(egui::RichText::new(pack_name).small().weak());
                            pack.clone_from(pack_id);
                        }
                        ui.horizontal(|ui| {
                            let mut on = app.bake_set.contains(id);
                            if ui
                                .checkbox(&mut on, "")
                                .on_hover_text("Include this clip in Bake")
                                .changed()
                            {
                                toggled = Some(id.clone());
                            }
                            ui.add_enabled_ui(fitted && !busy, |ui| {
                                if ui
                                    .selectable_label(&app.clip == id, name)
                                    // The raw name is what Quaternius' own
                                    // animation viewer and docs use.
                                    .on_hover_text(format!("Play {id}"))
                                    .clicked()
                                {
                                    play = Some(id.clone());
                                }
                            });
                        });
                    }
                });
        });

    if let Some(id) = toggled
        && !app.bake_set.remove(&id)
    {
        app.bake_set.insert(id);
    }
    if let Some(id) = play {
        app.clip = id.clone();
        app.preview_clip(&id);
    }

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let mut packs: Vec<&str> = Vec::new();
        for c in &app.clip_catalog {
            if !packs.contains(&c.pack_id.as_str()) {
                packs.push(&c.pack_id);
            }
        }
        let summary = format!(
            "{} pack{} / {} clips",
            packs.len(),
            if packs.len() == 1 { "" } else { "s" },
            app.clip_catalog.len()
        );
        ui.label(egui::RichText::new(summary).small().weak());
        render_install_pack_button(app, ui);
    });
}

/// Install another animation library from a Quaternius download.
///
/// The same flow serves the free Standard tiers and the paid Source ones:
/// point it at the zip or folder and the installer finds the library itself.
fn render_install_pack_button(app: &mut App, ui: &mut egui::Ui) {
    ui.add_enabled_ui(!app.workbench_busy(), |ui| {
        ui.menu_button("Add pack\u{2026}", |ui| {
            if ui
                .button("From archive or file\u{2026}")
                .on_hover_text("The .zip straight from the download, or a .glb / .gltf")
                .clicked()
            {
                if let Some(file) = rfd::FileDialog::new()
                    .set_title("Animation pack")
                    .add_filter("Animation pack", &["zip", "glb", "gltf"])
                    .pick_file()
                {
                    app.install_clip_pack(file);
                }
                ui.close();
            }
            if ui
                .button("From folder\u{2026}")
                .on_hover_text("A download you already extracted")
                .clicked()
            {
                if let Some(dir) = rfd::FileDialog::new()
                    .set_title("Animation pack folder")
                    .pick_folder()
                {
                    app.install_clip_pack(dir);
                }
                ui.close();
            }
            ui.separator();
            if ui
                .add_enabled(
                    !app.clip_packs_downloading(),
                    egui::Button::new("Download free packs\u{2026}"),
                )
                .on_hover_text("Quaternius Standard (CC0) from the latest Asset Tap release")
                .clicked()
            {
                app.request_clip_packs_download();
                ui.close();
            }
            render_quaternius_pack_menu_links(app, ui);
        })
        .response
        .on_hover_text("Install a Quaternius library. The library is found for you.");
    });
}

fn render_quaternius_pack_links(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("Quaternius (CC0):").small().weak());
        if ui
            .link(egui::RichText::new("Library").small())
            .on_hover_text(asset_tap_core::rig::UAL1_PAGE)
            .clicked()
        {
            crate::app::open_with_system(asset_tap_core::rig::UAL1_PAGE, Some(&mut app.toasts));
        }
        ui.label(egui::RichText::new("·").small().weak());
        if ui
            .link(egui::RichText::new("Library 2").small())
            .on_hover_text(asset_tap_core::rig::UAL2_PAGE)
            .clicked()
        {
            crate::app::open_with_system(asset_tap_core::rig::UAL2_PAGE, Some(&mut app.toasts));
        }
    });
}

fn render_quaternius_pack_menu_links(app: &mut App, ui: &mut egui::Ui) {
    if ui
        .button("Universal Animation Library\u{2026}")
        .on_hover_text(asset_tap_core::rig::UAL1_PAGE)
        .clicked()
    {
        crate::app::open_with_system(asset_tap_core::rig::UAL1_PAGE, Some(&mut app.toasts));
        ui.close();
    }
    if ui
        .button("Universal Animation Library 2\u{2026}")
        .on_hover_text(asset_tap_core::rig::UAL2_PAGE)
        .clicked()
    {
        crate::app::open_with_system(asset_tap_core::rig::UAL2_PAGE, Some(&mut app.toasts));
        ui.close();
    }
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

/// Render the model action buttons (Open GLB, Export GLB) with path display.
fn render_model_action_buttons(
    app: &mut App,
    ui: &mut egui::Ui,
    path: &std::path::Path,
    output: &asset_tap_core::types::PipelineOutput,
) {
    // Clone paths before entering closure to avoid borrow conflicts
    let glb_path = output.model_path.clone();
    let path_display = date_relative_path(path);

    // This row is rendered from two places: the loaded-model view, twelve
    // closures deep, and `render_model_info_fallback`, at its own top level.
    // Same pixels, different parent `Ui`, and `push_id` is *relative*, so the
    // salt alone gave the row a different id depending on which path drew it.
    // Switching between them (startup does: no model, then model) kept the
    // rect and changed the id, which is exactly what egui's
    // `warn_if_rect_changes_id` exists to catch, and it is not cosmetic: drag,
    // focus and animation state are keyed by id, so they are dropped on the
    // frame it changes.
    //
    // `UiBuilder::id` is the documented answer: it sets `global_scope`, so the
    // child's id is the given one outright rather than `parent.id.with(salt)`,
    // "this way child widgets can be moved in the ui tree without losing
    // state". Its precondition holds here: the two call sites are mutually
    // exclusive within a frame, so the row is never drawn twice.
    ui.scope_builder(
        egui::UiBuilder::new().id(egui::Id::new("model_action_buttons")),
        |ui| {
            ui.horizontal(|ui| {
                if ui
                    .button(format!("{} Open GLB", icons::EXTERNAL_LINK))
                    .on_hover_text("Open with system default viewer")
                    .clicked()
                {
                    crate::app::open_with_system(path, Some(&mut app.toasts));
                }

                if let Some(ref glb) = glb_path
                    && glb != path
                    && ui.button(format!("{} Open GLB", icons::FILE)).clicked()
                {
                    crate::app::open_with_system(glb, Some(&mut app.toasts));
                }

                if ui
                    .button(format!("{} Export GLB", icons::DOWNLOAD))
                    .on_hover_text("Save a copy to a custom location")
                    .clicked()
                    && let Some(dest) = rfd::FileDialog::new()
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

                ui.label(egui::RichText::new(path_display).small().secondary());
            });
        },
    );
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

        // Texture paths (typically just 2: Image_0.png and Image_1.png) are
        // scanned once per directory change by the cache, not re-read every
        // frame. Clone the small list so the cache can be borrowed mutably
        // below when fetching thumbnails.
        let texture_paths: Vec<_> = app.texture_cache.texture_paths().to_vec();

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
                && let Some(dest) = rfd::FileDialog::new()
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
                        "💡 Textures are extracted from the model's embedded images",
                        "This model has none, or they are referenced as external files",
                    ],
                    icons::WARNING,
                );
            } else {
                render_empty_state(ui, "No textures found in this model");
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
