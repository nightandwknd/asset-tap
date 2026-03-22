//! Asset Tap GUI
//!
//! A cross-platform GUI for generating 3D models from text prompts.

// Hide the console window on Windows release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
pub mod constants;
pub mod icons;
pub mod style;
mod texture_cache;
mod viewer;
mod views;

use asset_tap_core::constants::files::{APP_DISPLAY_NAME, APP_ID};
use eframe::egui;

/// Embedded app icon (tap graphic only, no text - 512x512).
const ICON_BYTES: &[u8] = include_bytes!("../../assets/icon.png");

/// Load the app icon from embedded bytes.
fn load_icon() -> egui::IconData {
    let image = image::load_from_memory(ICON_BYTES)
        .expect("Failed to load icon")
        .into_rgba8();
    let (width, height) = image.dimensions();
    egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    }
}

fn main() -> eframe::Result<()> {
    // Load .env file
    dotenvy::dotenv().ok();

    // Initialize tracing with dual output: console + rolling log file
    let _guard = asset_tap_core::error_log::init_tracing();

    // Load window icon
    let icon_data = load_icon();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title(APP_DISPLAY_NAME)
            .with_app_id(APP_ID)
            .with_icon(icon_data),
        ..Default::default()
    };

    eframe::run_native(
        APP_DISPLAY_NAME,
        options,
        Box::new(|cc| {
            // Configure fonts to include Phosphor icons
            let mut fonts = egui::FontDefinitions::default();
            egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
            cc.egui_ctx.set_fonts(fonts);

            // Install image loaders for egui
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(app::App::new(cc)))
        }),
    )
}
