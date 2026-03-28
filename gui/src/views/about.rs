//! About modal dialog.

use crate::style::RichTextExt;
use asset_tap_core::constants::files::APP_DISPLAY_NAME;
use chrono::Datelike;
use eframe::egui;

/// State for the About modal.
pub struct AboutModal {
    /// Whether the modal is open.
    pub is_open: bool,
}

impl AboutModal {
    pub fn new() -> Self {
        Self { is_open: false }
    }

    /// Open the about modal.
    pub fn open(&mut self) {
        self.is_open = true;
    }

    /// Close the about modal.
    pub fn close(&mut self) {
        self.is_open = false;
    }

    /// Render the about modal.
    ///
    /// `logo_texture` - Optional app logo texture to display
    pub fn render(&mut self, ctx: &egui::Context, logo_texture: Option<&egui::TextureHandle>) {
        if !self.is_open {
            return;
        }

        let mut should_close = false;

        // Draw backdrop
        egui::Area::new(egui::Id::new("about_backdrop"))
            .fixed_pos(egui::pos2(0.0, 0.0))
            .order(egui::Order::Background)
            .show(ctx, |ui| {
                let screen_rect = ctx.screen_rect();
                if ui
                    .allocate_response(screen_rect.size(), egui::Sense::click())
                    .clicked()
                {
                    should_close = true;
                }
                ui.painter()
                    .rect_filled(screen_rect, 0, egui::Color32::from_black_alpha(180));
            });

        // Modal window
        egui::Window::new("About")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .fixed_size(egui::vec2(400.0, 0.0))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(16.0);

                    // App logo
                    if let Some(logo) = logo_texture {
                        ui.image((logo.id(), egui::vec2(96.0, 96.0)));
                        ui.add_space(8.0);
                    }

                    // App title and version
                    ui.heading(egui::RichText::new(APP_DISPLAY_NAME).size(24.0));
                    ui.label(
                        egui::RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION")))
                            .small()
                            .secondary(),
                    );

                    ui.add_space(8.0);

                    ui.label(
                        egui::RichText::new("AI-powered text-to-3D model generation")
                            .italics()
                            .color(egui::Color32::from_rgb(150, 150, 150)),
                    );

                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(12.0);

                    // Creator info with clickable link (manually centered)
                    ui.horizontal(|ui| {
                        // Calculate total width needed for the content
                        let text1_width = ui.fonts(|f| {
                            f.layout_no_wrap(
                                "Created by ".to_string(),
                                egui::FontId::default(),
                                egui::Color32::GRAY,
                            )
                            .size()
                            .x
                        });
                        let link_width = ui.fonts(|f| {
                            f.layout_no_wrap(
                                "nightandwknd".to_string(),
                                egui::FontId::default(),
                                egui::Color32::WHITE,
                            )
                            .size()
                            .x
                        });
                        let total_width = text1_width + link_width + ui.spacing().item_spacing.x;

                        // Add spacing to center the content
                        let available_width = ui.available_width();
                        let left_padding = (available_width - total_width) / 2.0;
                        ui.add_space(left_padding);

                        ui.label(egui::RichText::new("Created by").secondary());
                        if ui
                            .link(egui::RichText::new("nightandwknd").strong())
                            .on_hover_text("Visit assettap.dev")
                            .clicked()
                        {
                            crate::app::open_with_system("https://assettap.dev", None);
                        }
                    });

                    ui.add_space(12.0);

                    // Tech stack badges
                    ui.label(egui::RichText::new("Built with").small().secondary());
                    ui.add_space(6.0);

                    // All badges on one line (manually centered)
                    let badge_color = egui::Color32::from_rgb(60, 60, 70);
                    ui.horizontal(|ui| {
                        // Calculate approximate width of all badges
                        // Each badge is roughly 50-70px, with spacing between
                        let badge_texts = [
                            " Rust ",
                            " egui ",
                            " three-d ",
                            " tokio ",
                            " glow ",
                            " reqwest ",
                            " serde ",
                        ];
                        let mut total_width = 0.0;
                        for text in &badge_texts {
                            let width = ui.fonts(|f| {
                                f.layout_no_wrap(
                                    text.to_string(),
                                    egui::FontId::proportional(10.0), // small() size
                                    egui::Color32::WHITE,
                                )
                                .size()
                                .x
                            });
                            total_width += width + ui.spacing().item_spacing.x;
                        }

                        // Add spacing to center the badges
                        let available_width = ui.available_width();
                        let left_padding = (available_width - total_width) / 2.0;
                        ui.add_space(left_padding);

                        // Add all badges
                        ui.label(
                            egui::RichText::new(" Rust ")
                                .small()
                                .background_color(badge_color),
                        );
                        ui.label(
                            egui::RichText::new(" egui ")
                                .small()
                                .background_color(badge_color),
                        );
                        ui.label(
                            egui::RichText::new(" three-d ")
                                .small()
                                .background_color(badge_color),
                        );
                        ui.label(
                            egui::RichText::new(" tokio ")
                                .small()
                                .background_color(badge_color),
                        );
                        ui.label(
                            egui::RichText::new(" glow ")
                                .small()
                                .background_color(badge_color),
                        );
                        ui.label(
                            egui::RichText::new(" reqwest ")
                                .small()
                                .background_color(badge_color),
                        );
                        ui.label(
                            egui::RichText::new(" serde ")
                                .small()
                                .background_color(badge_color),
                        );
                    });

                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(12.0);

                    // GitHub link
                    if ui
                        .link("View on GitHub")
                        .on_hover_text("https://github.com/nightandwknd/asset-tap")
                        .clicked()
                    {
                        crate::app::open_with_system(
                            "https://github.com/nightandwknd/asset-tap",
                            None,
                        );
                    }

                    ui.add_space(12.0);

                    // License info
                    ui.label(
                        egui::RichText::new("Licensed under AGPL-3.0")
                            .small()
                            .secondary(),
                    );

                    ui.add_space(4.0);

                    // Copyright with dynamic year
                    let current_year = chrono::Local::now().year();
                    ui.label(
                        egui::RichText::new(format!(
                            "© {} nightandwknd. All rights reserved.",
                            current_year
                        ))
                        .small()
                        .secondary(),
                    );

                    ui.add_space(16.0);

                    // Close button
                    if ui.button("Close").clicked() {
                        should_close = true;
                    }

                    ui.add_space(8.0);
                });
            });

        if should_close {
            self.close();
        }
    }
}

impl Default for AboutModal {
    fn default() -> Self {
        Self::new()
    }
}
