//! Dev tool: refresh provider models from discovery APIs.
//!
//! This queries provider APIs to discover available models and caches the results.
//! Used during development to evaluate newly available models.
//!
//! Run with:
//! ```bash
//! export FAL_KEY="your-fal-key"
//! cargo run --bin refresh-models -p asset-tap-core
//! ```

use asset_tap_core::providers::{ProviderCapability, ProviderRegistry};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load .env for API keys
    dotenvy::dotenv().ok();

    println!("🔄 Refreshing models from provider APIs...\n");

    let registry = ProviderRegistry::new();

    // Force refresh — bypass cache
    if let Err(e) = registry.refresh_discovery_blocking(true) {
        eprintln!("❌ Discovery refresh failed: {}", e);
        std::process::exit(1);
    }

    println!("\n✅ Discovery complete\n");

    // Print discovered models
    let providers = registry.list_available();
    for provider in &providers {
        let metadata = provider.metadata();
        println!("{} ({})", metadata.name, metadata.id);

        let t2i = provider.list_models(ProviderCapability::TextToImage);
        if !t2i.is_empty() {
            println!("  Text-to-Image ({}):", t2i.len());
            for model in &t2i {
                let default_marker = if model.is_default { " *" } else { "" };
                println!(
                    "    {} {}{}",
                    if model.is_default { "●" } else { "○" },
                    model.id,
                    default_marker
                );
            }
        }

        let i3d = provider.list_models(ProviderCapability::ImageTo3D);
        if !i3d.is_empty() {
            println!("  Image-to-3D ({}):", i3d.len());
            for model in &i3d {
                let default_marker = if model.is_default { " *" } else { "" };
                println!(
                    "    {} {}{}",
                    if model.is_default { "●" } else { "○" },
                    model.id,
                    default_marker
                );
            }
        }
        println!();
    }

    Ok(())
}
