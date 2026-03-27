//! Interactive walkthrough overlay for new users.
//!
//! Guides users through key UI elements with a spotlight overlay
//! and floating tooltip cards. Triggered automatically on first run
//! (after the welcome modal closes) and re-accessible from Help menu.

use crate::icons;
use eframe::egui;
use std::collections::HashMap;

/// Walkthrough step targets, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WalkthroughStep {
    SidebarPanel,
    PromptInput,
    TemplateSelector,
    ImageDropZone,
    ProvidersSection,
    GenerateButton,
    PreviewPanel,
}

/// Ordered list of walkthrough steps.
const STEPS: &[WalkthroughStep] = &[
    WalkthroughStep::SidebarPanel,
    WalkthroughStep::PromptInput,
    WalkthroughStep::TemplateSelector,
    WalkthroughStep::ImageDropZone,
    WalkthroughStep::ProvidersSection,
    WalkthroughStep::GenerateButton,
    WalkthroughStep::PreviewPanel,
];

struct StepContent {
    title: &'static str,
    body: &'static str,
}

fn step_content(step: WalkthroughStep) -> StepContent {
    match step {
        WalkthroughStep::SidebarPanel => StepContent {
            title: "Control Panel",
            body: "This is where you configure and launch your 3D generation. All inputs and settings live here.",
        },
        WalkthroughStep::PromptInput => StepContent {
            title: "Prompt",
            body: "Describe what you want to create — a character, prop, or scene. Be descriptive for best results.",
        },
        WalkthroughStep::TemplateSelector => StepContent {
            title: "Templates",
            body: "Templates add structured phrasing to your prompt. Great for consistent character or prop styles.",
        },
        WalkthroughStep::ImageDropZone => StepContent {
            title: "Input Image",
            body: "Optionally provide an existing image to skip image generation and go straight to 3D conversion.",
        },
        WalkthroughStep::ProvidersSection => StepContent {
            title: "Providers & Models",
            body: "Choose your AI provider and model for each generation step. Configure API keys in Settings first.",
        },
        WalkthroughStep::GenerateButton => StepContent {
            title: "Generate",
            body: "Once your prompt and API keys are ready, click here to start the text-to-image-to-3D pipeline.",
        },
        WalkthroughStep::PreviewPanel => StepContent {
            title: "Preview",
            body: "Results appear here. Switch between Image, 3D Model, and Textures tabs to inspect your generation.",
        },
    }
}

/// Interactive walkthrough state.
pub struct Walkthrough {
    /// Whether the walkthrough is currently active.
    pub is_active: bool,
    /// Current step index.
    current_step: usize,
    /// Captured widget rects from the previous frame.
    rects: HashMap<WalkthroughStep, egui::Rect>,
    /// Frames since activation (ignores clicks on first few frames so the
    /// menu click that started the tour doesn't immediately dismiss it).
    frames_active: u32,
}

impl Walkthrough {
    pub fn new() -> Self {
        Self {
            is_active: false,
            current_step: 0,
            rects: HashMap::new(),
            frames_active: 0,
        }
    }

    /// Start the walkthrough from the beginning.
    pub fn start(&mut self) {
        self.is_active = true;
        self.current_step = 0;
        self.rects.clear();
        self.frames_active = 0;
    }

    /// Register a widget rect for a walkthrough step.
    /// Call this during normal widget rendering.
    pub fn register_rect(&mut self, step: WalkthroughStep, rect: egui::Rect) {
        if self.is_active {
            self.rects.insert(step, rect);
        }
    }

    /// Render the walkthrough overlay. Call at the end of `update()`.
    pub fn render(&mut self, ctx: &egui::Context) {
        if !self.is_active {
            return;
        }

        self.frames_active += 1;

        let current = match STEPS.get(self.current_step) {
            Some(s) => *s,
            None => {
                self.is_active = false;
                return;
            }
        };

        let target_rect = match self.rects.get(&current).copied() {
            Some(r) => r,
            None => {
                // Rect not captured yet (first frame) — wait for next frame.
                ctx.request_repaint();
                return;
            }
        };

        let screen = ctx.screen_rect();
        let time = ctx.input(|i| i.time) as f32;
        let pulse = (time * 2.5).sin() * 0.5 + 0.5; // 0..1

        // Expand target rect for padding, then clamp to screen so the
        // pulsing border is fully visible even for edge-touching panels.
        let highlight = target_rect.expand(8.0).intersect(screen.shrink(4.0));

        // -- Overlay with cutout --
        let overlay_color = egui::Color32::from_black_alpha(160);

        egui::Area::new(egui::Id::new("walkthrough_overlay"))
            .fixed_pos(egui::pos2(0.0, 0.0))
            .order(egui::Order::Tooltip)
            .interactable(false)
            .show(ctx, |ui| {
                let painter = ui.painter();

                // Top
                painter.rect_filled(
                    egui::Rect::from_min_max(screen.min, egui::pos2(screen.max.x, highlight.min.y)),
                    0,
                    overlay_color,
                );
                // Bottom
                painter.rect_filled(
                    egui::Rect::from_min_max(egui::pos2(screen.min.x, highlight.max.y), screen.max),
                    0,
                    overlay_color,
                );
                // Left
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(screen.min.x, highlight.min.y),
                        egui::pos2(highlight.min.x, highlight.max.y),
                    ),
                    0,
                    overlay_color,
                );
                // Right
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(highlight.max.x, highlight.min.y),
                        egui::pos2(screen.max.x, highlight.max.y),
                    ),
                    0,
                    overlay_color,
                );

                // Pulsing border around the cutout.
                let border_alpha = (180.0 + pulse * 75.0) as u8;
                let border_color =
                    egui::Color32::from_rgba_unmultiplied(100, 180, 255, border_alpha);
                painter.rect_stroke(
                    highlight,
                    4,
                    egui::Stroke::new(2.0 + pulse, border_color),
                    egui::StrokeKind::Outside,
                );
            });

        // -- Tooltip card state --
        let content = step_content(current);
        let tooltip_width = 300.0;
        let tooltip_pos = compute_tooltip_pos(highlight, screen, tooltip_width);

        let step_num = self.current_step;
        let total = STEPS.len();
        let is_last = step_num + 1 == total;

        let mut advance = false;
        let mut go_back = false;
        let mut dismiss = false;

        // -- Input blocker (non-interactive overlay that prevents underlying widgets
        // from receiving hover/focus). We intentionally do NOT use Sense::click() here
        // because a full-screen clickable rect would consume click events before the
        // tooltip card buttons can process them. Instead, dismiss-on-outside-click is
        // handled below via ctx.input() pointer state.
        egui::Area::new(egui::Id::new("walkthrough_blocker"))
            .fixed_pos(egui::pos2(0.0, 0.0))
            .order(egui::Order::Tooltip)
            .interactable(false)
            .show(ctx, |ui| {
                ui.allocate_rect(screen, egui::Sense::hover());
            });

        // -- Tooltip card --
        egui::Area::new(egui::Id::new("walkthrough_tooltip"))
            .fixed_pos(tooltip_pos)
            .order(egui::Order::Tooltip)
            .show(ctx, |ui| {
                egui::Frame::window(ui.style())
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 4],
                        blur: 12,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(80),
                    })
                    .show(ui, |ui| {
                        ui.set_width(tooltip_width);

                        // Step counter
                        ui.label(
                            egui::RichText::new(format!("Step {} of {}", step_num + 1, total))
                                .size(11.0)
                                .weak(),
                        );
                        ui.add_space(4.0);

                        // Title
                        ui.label(egui::RichText::new(content.title).size(15.0).strong());
                        ui.add_space(6.0);

                        // Body
                        ui.label(egui::RichText::new(content.body).size(13.0));
                        ui.add_space(12.0);

                        // Navigation
                        ui.horizontal(|ui| {
                            if step_num > 0
                                && ui.button(format!("{} Back", icons::ARROW_LEFT)).clicked()
                            {
                                go_back = true;
                            }

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .button(if is_last {
                                            "Done".to_string()
                                        } else {
                                            format!("Next {}", icons::ARROW_RIGHT)
                                        })
                                        .clicked()
                                    {
                                        advance = true;
                                    }
                                    if !is_last
                                        && ui
                                            .button(egui::RichText::new("Skip tour").size(12.0))
                                            .clicked()
                                    {
                                        dismiss = true;
                                    }
                                },
                            );
                        });
                    });
            });

        // Escape to dismiss
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            dismiss = true;
        }

        // Arrow keys for navigation
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
            advance = true;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) && step_num > 0 {
            go_back = true;
        }

        if dismiss {
            self.is_active = false;
            self.current_step = 0;
        } else if advance {
            self.current_step += 1;
            if self.current_step >= STEPS.len() {
                self.is_active = false;
                self.current_step = 0;
            }
            ctx.request_repaint();
        } else if go_back {
            self.current_step = self.current_step.saturating_sub(1);
            ctx.request_repaint();
        }

        // Keep repainting for the pulse animation.
        ctx.request_repaint();
    }
}

/// Compute the best position for the tooltip relative to the highlighted region.
fn compute_tooltip_pos(highlight: egui::Rect, screen: egui::Rect, width: f32) -> egui::Pos2 {
    let height_est = 180.0;
    let margin = 16.0;

    // Prefer right, then left, then below, then above.
    let right_x = highlight.max.x + margin;
    if right_x + width < screen.max.x {
        return egui::pos2(
            right_x,
            (highlight.center().y - height_est / 2.0).clamp(margin, screen.max.y - height_est),
        );
    }

    let left_x = highlight.min.x - width - margin;
    if left_x > screen.min.x {
        return egui::pos2(
            left_x,
            (highlight.center().y - height_est / 2.0).clamp(margin, screen.max.y - height_est),
        );
    }

    let below_y = highlight.max.y + margin;
    if below_y + height_est < screen.max.y {
        return egui::pos2(
            (highlight.center().x - width / 2.0).clamp(margin, screen.max.x - width - margin),
            below_y,
        );
    }

    // Above
    egui::pos2(
        (highlight.center().x - width / 2.0).clamp(margin, screen.max.x - width - margin),
        (highlight.min.y - height_est - margin).max(margin),
    )
}
