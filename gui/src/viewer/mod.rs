//! 3D model and image viewers.
//!
//! This module provides viewers for previewing generated assets:
//! - Model info display with external viewer launch
//! - Image viewer via egui's built-in image support
//! - GLB WebP texture converter for compatibility

pub mod glb_webp;
pub mod model;

// Note: image viewing is handled by egui's built-in Image widget
// using file:// URIs with egui_extras image loaders
