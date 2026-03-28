//! Library browser for viewing and selecting past generations.
//!
//! Provides a modal dialog to browse images and models in the output/ directory.

use crate::icons;
use crate::style::RichTextExt;
use asset_tap_core::constants::files::bundle as bundle_files;
use asset_tap_core::settings::get_output_dir;
use eframe::egui;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;

/// Thumbnail data loaded in background
struct ThumbnailData {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}

/// Message sent from background loader to main thread
enum ThumbnailMessage {
    Loaded(PathBuf, ThumbnailData),
    Failed(PathBuf),
}

/// Asset type for library browsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetType {
    /// Browse images only.
    Images,
    /// Browse 3D models only.
    Models,
    /// Browse texture directories.
    Textures,
    /// Browse all assets.
    All,
}

/// Selection mode for the library browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMode {
    /// Single selection, returns when item is picked.
    Single,
    /// Multiple selection with confirm button.
    Multiple,
}

/// A library item representing a generated asset.
#[derive(Debug, Clone)]
pub struct LibraryItem {
    /// Full path to the asset.
    pub path: PathBuf,
    /// Display name (filename).
    pub name: String,
    /// Timestamp extracted from filename.
    pub timestamp: String,
    /// File size in bytes.
    pub size: u64,
    /// Asset type (image or model).
    pub asset_type: AssetType,
    /// Custom name from bundle.json (if available).
    pub custom_name: Option<String>,
    /// Prompt from bundle.json (if available).
    pub prompt: Option<String>,
    /// 3D model name from bundle.json (if available).
    pub model_name: Option<String>,
}

impl LibraryItem {
    /// Format the file size for display.
    pub fn formatted_size(&self) -> String {
        use asset_tap_core::constants::http::{BYTES_PER_KB, BYTES_PER_MB};

        if self.size < BYTES_PER_KB {
            format!("{} B", self.size)
        } else if self.size < BYTES_PER_MB {
            format!("{:.1} KB", self.size as f64 / BYTES_PER_KB as f64)
        } else {
            format!("{:.1} MB", self.size as f64 / BYTES_PER_MB as f64)
        }
    }

    /// Format the timestamp for display.
    pub fn formatted_timestamp(&self) -> String {
        // Timestamp format: YYYY-MM-DD_HHMMSS (17 chars)
        if self.timestamp.len() == 17 {
            let date = &self.timestamp[0..10]; // YYYY-MM-DD
            let time = &self.timestamp[11..17]; // HHMMSS
            format!("{} {}:{}:{}", date, &time[0..2], &time[2..4], &time[4..6])
        } else {
            self.timestamp.clone()
        }
    }

    /// Get the display name for this item (custom name > prompt > filename).
    pub fn display_name(&self) -> &str {
        self.custom_name
            .as_deref()
            .or(self.prompt.as_deref())
            .unwrap_or(&self.name)
    }

    /// Get a subtitle with additional info (model name and/or file type).
    pub fn subtitle(&self) -> Option<String> {
        match (self.model_name.as_ref(), &self.asset_type) {
            (Some(model), AssetType::Models) => Some(format!("{} • GLB", model)),
            (Some(model), AssetType::Images) => Some(model.clone()),
            (None, AssetType::Models) => Some("GLB".to_string()),
            _ => None,
        }
    }
}

/// Library browser state.
pub struct LibraryBrowser {
    /// Whether the browser is open.
    pub is_open: bool,
    /// What type of assets to show.
    pub asset_type: AssetType,
    /// Selection mode.
    pub selection_mode: SelectionMode,
    /// Cached list of items.
    items: Vec<LibraryItem>,
    /// Whether the cache needs refresh.
    needs_refresh: bool,
    /// Currently selected items (for multiple selection).
    selected: Vec<PathBuf>,
    /// Search/filter text.
    filter: String,
    /// Callback identifier for what action triggered the browser.
    pub callback_id: Option<String>,
    /// Set of image paths that have been requested for loading.
    loading_requested: HashSet<PathBuf>,
    /// Sender to request thumbnail loading
    thumb_tx: Sender<PathBuf>,
    /// Receiver for loaded thumbnails
    thumb_rx: Receiver<ThumbnailMessage>,
    /// Cache of loaded thumbnail texture handles
    thumb_cache: HashMap<PathBuf, egui::TextureHandle>,
    /// Paths that failed to load
    thumb_failed: HashSet<PathBuf>,
    /// Output directory to scan (from settings).
    output_dir: Option<PathBuf>,
    /// Receiver for async library refresh results.
    refresh_rx: Option<Receiver<Vec<LibraryItem>>>,
    /// Whether library is currently refreshing in the background.
    is_refreshing: bool,
}

impl Default for LibraryBrowser {
    fn default() -> Self {
        Self::new()
    }
}

impl LibraryBrowser {
    pub fn new() -> Self {
        // Create channel for thumbnail loading
        let (request_tx, request_rx) = channel::<PathBuf>();
        let (result_tx, result_rx) = channel::<ThumbnailMessage>();

        // Spawn background thumbnail loader thread
        thread::spawn(move || {
            // Target thumbnail size
            const THUMB_SIZE: u32 = 120;

            while let Ok(path) = request_rx.recv() {
                // Load and resize image in background (catch panics to avoid killing thread)
                let path_clone = path.clone();
                let result = std::panic::catch_unwind(|| {
                    // Use with_guessed_format() to detect actual format from file
                    // content (magic bytes), not just extension. This handles cases
                    // like JPEG files saved with a .png extension.
                    let img = image::ImageReader::open(&path_clone)?
                        .with_guessed_format()?
                        .decode()?;
                    let thumb = img.thumbnail(THUMB_SIZE, THUMB_SIZE);
                    let rgba = thumb.to_rgba8();
                    let (width, height) = rgba.dimensions();
                    Ok::<_, image::ImageError>(ThumbnailData {
                        pixels: rgba.into_raw(),
                        width,
                        height,
                    })
                });

                match result {
                    Ok(Ok(data)) => {
                        let _ = result_tx.send(ThumbnailMessage::Loaded(path, data));
                    }
                    Ok(Err(e)) => {
                        tracing::warn!("Thumbnail load error for {:?}: {}", path, e);
                        let _ = result_tx.send(ThumbnailMessage::Failed(path));
                    }
                    Err(_) => {
                        tracing::error!("Thumbnail thread panic for {:?}", path);
                        let _ = result_tx.send(ThumbnailMessage::Failed(path));
                    }
                }
            }
        });

        Self {
            is_open: false,
            asset_type: AssetType::All,
            selection_mode: SelectionMode::Single,
            items: Vec::new(),
            needs_refresh: true,
            selected: Vec::new(),
            filter: String::new(),
            callback_id: None,
            loading_requested: HashSet::new(),
            thumb_tx: request_tx,
            thumb_rx: result_rx,
            thumb_cache: HashMap::new(),
            thumb_failed: HashSet::new(),
            output_dir: None,
            refresh_rx: None,
            is_refreshing: false,
        }
    }

    /// Set the output directory to scan.
    pub fn set_output_dir(&mut self, dir: PathBuf) {
        if self.output_dir.as_ref() != Some(&dir) {
            self.output_dir = Some(dir);
            self.needs_refresh = true;
        }
    }

    /// Open the browser for selecting images.
    pub fn open_for_images(&mut self, callback_id: &str) {
        self.is_open = true;
        self.asset_type = AssetType::Images;
        self.selection_mode = SelectionMode::Single;
        self.needs_refresh = true;
        self.selected.clear();
        self.filter.clear();
        self.callback_id = Some(callback_id.to_string());
    }

    /// Open the browser for selecting models.
    pub fn open_for_models(&mut self, callback_id: &str) {
        self.is_open = true;
        self.asset_type = AssetType::Models;
        self.selection_mode = SelectionMode::Single;
        self.needs_refresh = true;
        self.selected.clear();
        self.filter.clear();
        self.callback_id = Some(callback_id.to_string());
    }

    /// Open the browser for selecting texture directories.
    pub fn open_for_textures(&mut self, callback_id: &str) {
        self.is_open = true;
        self.asset_type = AssetType::Textures;
        self.selection_mode = SelectionMode::Single;
        self.needs_refresh = true;
        self.selected.clear();
        self.filter.clear();
        self.callback_id = Some(callback_id.to_string());
    }

    /// Close the browser.
    /// Note: callback_id is preserved until the next open() call so that
    /// the caller can still read it after the browser closes.
    pub fn close(&mut self) {
        self.is_open = false;
        // Don't clear callback_id here - it's needed by handle_library_selection
        // which runs after render() returns. It gets cleared on next open.
    }

    /// Refresh the item list.
    /// Start async refresh of library items in the background.
    pub fn start_async_refresh(&mut self) {
        // Don't start if already refreshing
        if self.is_refreshing {
            return;
        }

        self.is_refreshing = true;

        // Use configured output_dir if set, otherwise fall back to get_output_dir()
        let default_output = get_output_dir();
        let output_dir = self.output_dir.clone().unwrap_or(default_output);
        let asset_type = self.asset_type;

        // Create channel for results
        let (tx, rx) = channel();
        self.refresh_rx = Some(rx);

        // Spawn background thread to scan directories
        thread::spawn(move || {
            let items = Self::scan_library_items(&output_dir, asset_type);
            let _ = tx.send(items);
        });
    }

    /// Poll for async refresh completion.
    pub fn poll_refresh(&mut self) {
        if !self.is_refreshing {
            return;
        }

        let rx = match self.refresh_rx.as_ref() {
            Some(rx) => rx,
            None => {
                self.is_refreshing = false;
                return;
            }
        };

        // Try to receive (non-blocking)
        match rx.try_recv() {
            Ok(items) => {
                // Success! Update items
                self.items = items;
                self.is_refreshing = false;
                self.needs_refresh = false;
                self.refresh_rx = None;
                // Clear stale loading requests so thumbnails can be re-requested
                self.loading_requested.clear();
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                // Still loading, do nothing
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                // Thread died unexpectedly
                self.is_refreshing = false;
                self.refresh_rx = None;
            }
        }
    }

    /// Scan library items on a background thread (blocking I/O).
    fn scan_library_items(output_dir: &PathBuf, asset_type: AssetType) -> Vec<LibraryItem> {
        let mut items = Vec::new();

        // Scan generation subdirectories in output_dir
        if let Ok(entries) = std::fs::read_dir(output_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // Scan files in the generation directory
                    if let Ok(files) = std::fs::read_dir(&path) {
                        for file_entry in files.flatten() {
                            let file_path = file_entry.path();

                            // Check for textures directory
                            if file_path.is_dir() {
                                if let Some(name) = file_path.file_name() {
                                    if name == bundle_files::TEXTURES_DIR
                                        && matches!(
                                            asset_type,
                                            AssetType::Textures | AssetType::All
                                        )
                                    {
                                        if let Some(item) =
                                            Self::create_textures_item_static(&file_path, &path)
                                        {
                                            items.push(item);
                                        }
                                    }
                                }
                                continue;
                            }

                            if let Some(ext) = file_path.extension() {
                                let ext = ext.to_string_lossy().to_lowercase();
                                let is_image =
                                    ext == "png" || ext == "jpg" || ext == "jpeg" || ext == "webp";
                                let is_model = ext == "glb" || ext == "gltf";

                                // Check for images
                                if is_image
                                    && matches!(asset_type, AssetType::Images | AssetType::All)
                                {
                                    if let Some(item) =
                                        Self::create_item_static(file_path, AssetType::Images)
                                    {
                                        items.push(item);
                                    }
                                }
                                // Check for models
                                else if is_model
                                    && matches!(asset_type, AssetType::Models | AssetType::All)
                                {
                                    if let Some(item) =
                                        Self::create_item_static(file_path, AssetType::Models)
                                    {
                                        items.push(item);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Sort by timestamp descending (newest first)
        items.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        items
    }

    /// Static version for background thread use.
    fn create_item_static(path: PathBuf, asset_type: AssetType) -> Option<LibraryItem> {
        Self::create_item_impl(path, asset_type)
    }

    fn create_item_impl(path: PathBuf, asset_type: AssetType) -> Option<LibraryItem> {
        let name = path.file_name()?.to_string_lossy().to_string();

        // Extract timestamp from parent directory name (YYYY-MM-DD_HHMMSS format)
        let timestamp = path
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .filter(|n| n.len() == 17 && n.chars().nth(10) == Some('_'))
            .unwrap_or_default();

        let size = std::fs::metadata(&path).ok()?.len();

        // Try to load bundle.json from parent directory
        let (custom_name, prompt, model_name) = path
            .parent()
            .and_then(|parent| {
                let bundle_path = parent.join(bundle_files::METADATA);
                std::fs::read_to_string(bundle_path).ok()
            })
            .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
            .map(|json| {
                let custom_name = json
                    .get("name")
                    .and_then(|n| n.as_str())
                    .map(|s| s.to_string());

                let prompt = json
                    .get("config")
                    .and_then(|c| c.get("prompt"))
                    .and_then(|p| p.as_str())
                    .map(|s| s.to_string());

                let model_name = json
                    .get("config")
                    .and_then(|c| c.get("model_3d"))
                    .and_then(|m| m.as_str())
                    .map(|s| s.to_string());

                (custom_name, prompt, model_name)
            })
            .unwrap_or((None, None, None));

        Some(LibraryItem {
            path,
            name,
            timestamp,
            size,
            asset_type,
            custom_name,
            prompt,
            model_name,
        })
    }

    /// Static version for background thread use.
    fn create_textures_item_static(
        textures_dir: &std::path::Path,
        generation_dir: &std::path::Path,
    ) -> Option<LibraryItem> {
        Self::create_textures_item_impl(textures_dir, generation_dir)
    }

    fn create_textures_item_impl(
        textures_dir: &std::path::Path,
        generation_dir: &std::path::Path,
    ) -> Option<LibraryItem> {
        // Single pass: count textures AND calculate total size
        let (texture_count, total_size) = std::fs::read_dir(textures_dir)
            .ok()?
            .flatten()
            .filter(|e| {
                e.path()
                    .extension()
                    .is_some_and(|ext| matches!(ext.to_str(), Some("png" | "jpg" | "jpeg")))
            })
            .filter_map(|e| e.metadata().ok().map(|m| m.len()))
            .fold((0usize, 0u64), |(count, size), file_size| {
                (count + 1, size + file_size)
            });

        if texture_count == 0 {
            return None;
        }

        // Extract timestamp from generation directory name
        let timestamp = generation_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .filter(|n| n.len() == 17 && n.chars().nth(10) == Some('_'))
            .unwrap_or_default();

        // Load bundle metadata if available
        let bundle_path = generation_dir.join(bundle_files::METADATA);
        let (custom_name, prompt, model_name) = std::fs::read_to_string(bundle_path)
            .ok()
            .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
            .map(|json| {
                let custom_name = json
                    .get("name")
                    .and_then(|n| n.as_str())
                    .map(|s| s.to_string());

                let prompt = json
                    .get("config")
                    .and_then(|c| c.get("prompt"))
                    .and_then(|p| p.as_str())
                    .map(|s| s.to_string());

                let model_name = json
                    .get("config")
                    .and_then(|c| c.get("model_3d"))
                    .and_then(|m| m.as_str())
                    .map(|s| s.to_string());

                (custom_name, prompt, model_name)
            })
            .unwrap_or((None, None, None));

        Some(LibraryItem {
            path: textures_dir.to_path_buf(),
            name: format!("{} textures", texture_count),
            timestamp,
            size: total_size,
            asset_type: AssetType::Textures,
            custom_name,
            prompt,
            model_name,
        })
    }

    /// Process any loaded thumbnails from background thread
    fn process_loaded_thumbnails(&mut self, ctx: &egui::Context) {
        // Process all available loaded thumbnails (non-blocking)
        while let Ok(msg) = self.thumb_rx.try_recv() {
            match msg {
                ThumbnailMessage::Loaded(path, data) => {
                    // Create texture from loaded data
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [data.width as usize, data.height as usize],
                        &data.pixels,
                    );
                    let texture = ctx.load_texture(
                        path.to_string_lossy(),
                        image,
                        egui::TextureOptions::default(),
                    );
                    self.thumb_cache.insert(path, texture);
                }
                ThumbnailMessage::Failed(path) => {
                    self.thumb_failed.insert(path);
                }
            }
        }
    }

    /// Request a thumbnail to be loaded in background
    fn request_thumbnail(&mut self, path: &PathBuf) {
        if !self.loading_requested.contains(path)
            && !self.thumb_cache.contains_key(path)
            && !self.thumb_failed.contains(path)
        {
            self.loading_requested.insert(path.clone());
            let _ = self.thumb_tx.send(path.clone());
        }
    }

    /// Render the library browser modal.
    /// Returns Some(paths) if selection was made, None otherwise.
    pub fn render(&mut self, ctx: &egui::Context) -> Option<Vec<PathBuf>> {
        if !self.is_open {
            return None;
        }

        // Process any thumbnails that finished loading
        self.process_loaded_thumbnails(ctx);

        // Poll for async library refresh completion
        self.poll_refresh();

        if self.needs_refresh {
            self.start_async_refresh();
            self.needs_refresh = false;
        }

        let mut result = None;
        let mut should_close = false;

        // Check for clicks outside the modal to close it
        let modal_id = egui::Id::new("library_modal");

        // Draw a semi-transparent backdrop that closes the modal when clicked
        egui::Area::new(modal_id.with("backdrop"))
            .fixed_pos(egui::pos2(0.0, 0.0))
            .order(egui::Order::Background)
            .show(ctx, |ui| {
                let screen_rect = ctx.content_rect();
                let response = ui.allocate_response(screen_rect.size(), egui::Sense::click());
                if response.clicked() {
                    should_close = true;
                }
                // Draw semi-transparent backdrop
                ui.painter()
                    .rect_filled(screen_rect, 0, egui::Color32::from_black_alpha(100));
            });

        // 5 icons: 140px each + 10px spacing = 740px content
        egui::Window::new("Asset Library")
            .collapsible(false)
            .resizable(true)
            .default_size([740.0, 500.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                // Header with filter and close button
                ui.horizontal(|ui| {
                    ui.heading(match self.asset_type {
                        AssetType::Images => "Select Image",
                        AssetType::Models => "Select Model",
                        AssetType::Textures => "Select Textures",
                        AssetType::All => "Asset Library",
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(format!("{} Close", icons::X)).clicked() {
                            should_close = true;
                        }

                        if ui
                            .button(format!("{} Refresh", icons::ARROWS_ROTATE))
                            .clicked()
                        {
                            self.needs_refresh = true;
                        }
                    });
                });

                ui.add_space(8.0);

                // Filter input
                ui.horizontal(|ui| {
                    ui.label("Filter:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.filter)
                            .hint_text("Search by filename...")
                            .desired_width(200.0),
                    );

                    if !self.filter.is_empty() && ui.small_button(icons::X).clicked() {
                        self.filter.clear();
                    }

                    let count = if self.filter.is_empty() {
                        self.items.len()
                    } else {
                        let filter_lower = self.filter.to_lowercase();
                        self.items
                            .iter()
                            .filter(|i| i.name.to_lowercase().contains(&filter_lower))
                            .count()
                    };
                    ui.label(egui::RichText::new(format!("{} items", count)).secondary());
                });

                ui.separator();

                // Filter items without cloning - cache the lowercase filter once
                let filter_lower = self.filter.to_lowercase();
                let matches_filter = |item: &LibraryItem| -> bool {
                    if filter_lower.is_empty() {
                        return true;
                    }

                    // Early return on first match to avoid unnecessary string operations
                    item.name.to_lowercase().contains(&filter_lower)
                        || item
                            .custom_name
                            .as_ref()
                            .is_some_and(|n| n.to_lowercase().contains(&filter_lower))
                        || item
                            .prompt
                            .as_ref()
                            .is_some_and(|p| p.to_lowercase().contains(&filter_lower))
                        || item
                            .model_name
                            .as_ref()
                            .is_some_and(|m| m.to_lowercase().contains(&filter_lower))
                };

                // Use iterator instead of collecting - no clones!
                let filtered_iter = self.items.iter().filter(|item| matches_filter(item));
                let has_filtered_items = filtered_iter.clone().next().is_some();

                if !has_filtered_items {
                    ui.vertical_centered(|ui| {
                        ui.add_space(50.0);
                        ui.label(
                            egui::RichText::new("No assets found")
                                .size(16.0)
                                .secondary()
                                .italics(),
                        );
                        ui.add_space(10.0);
                        ui.label(
                            egui::RichText::new("Generate some assets to see them here!")
                                .secondary(),
                        );
                    });
                } else {
                    // Track what was clicked to handle after rendering
                    let mut clicked_path: Option<PathBuf> = None;
                    let mut right_clicked_path: Option<PathBuf> = None;

                    // Collect paths to request loading (after the loop to avoid borrow issues)
                    let mut paths_to_request: Vec<PathBuf> = Vec::new();
                    let mut has_loading = false;

                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);

                                for item in self.items.iter().filter(|item| matches_filter(item)) {
                                    let is_selected = self.selected.contains(&item.path);

                                    // Get cached thumbnail if available
                                    let thumbnail = if item.asset_type == AssetType::Images {
                                        self.thumb_cache.get(&item.path)
                                    } else {
                                        None
                                    };

                                    // Request loading if needed
                                    if item.asset_type == AssetType::Images
                                        && thumbnail.is_none()
                                        && !self.loading_requested.contains(&item.path)
                                        && !self.thumb_failed.contains(&item.path)
                                    {
                                        paths_to_request.push(item.path.clone());
                                    }

                                    // Check if still loading
                                    let is_loading = item.asset_type == AssetType::Images
                                        && thumbnail.is_none()
                                        && self.loading_requested.contains(&item.path)
                                        && !self.thumb_failed.contains(&item.path);
                                    if is_loading {
                                        has_loading = true;
                                    }

                                    let response = render_library_item_cached(
                                        ui,
                                        item,
                                        is_selected,
                                        thumbnail,
                                        is_loading,
                                    );

                                    if response.clicked() {
                                        clicked_path = Some(item.path.clone());
                                    }

                                    if response.secondary_clicked() {
                                        right_clicked_path = Some(item.path.clone());
                                    }
                                }
                            });
                        });

                    // Request thumbnails to be loaded
                    for path in paths_to_request {
                        self.request_thumbnail(&path);
                        has_loading = true;
                    }

                    // Request repaint if thumbnails are still loading
                    if has_loading {
                        ui.ctx().request_repaint();
                    }

                    // Handle clicks after rendering
                    if let Some(path) = clicked_path {
                        match self.selection_mode {
                            SelectionMode::Single => {
                                result = Some(vec![path]);
                                should_close = true;
                            }
                            SelectionMode::Multiple => {
                                if self.selected.contains(&path) {
                                    self.selected.retain(|p| p != &path);
                                } else {
                                    self.selected.push(path);
                                }
                            }
                        }
                    }

                    if let Some(path) = right_clicked_path {
                        crate::app::open_with_system(&path, None);
                    }
                }

                // Footer with selection actions (for multiple selection mode)
                if self.selection_mode == SelectionMode::Multiple && !self.selected.is_empty() {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(format!("{} selected", self.selected.len()));

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("✓ Confirm Selection").clicked() {
                                result = Some(self.selected.clone());
                                should_close = true;
                            }

                            if ui.button("Clear Selection").clicked() {
                                self.selected.clear();
                            }
                        });
                    });
                }
            });

        if should_close {
            self.close();
        }

        result
    }
}

/// Render a single library item in the grid with cached thumbnail.
fn render_library_item_cached(
    ui: &mut egui::Ui,
    item: &LibraryItem,
    is_selected: bool,
    thumbnail: Option<&egui::TextureHandle>,
    is_loading: bool,
) -> egui::Response {
    // Taller to fit full filename with wrap
    let item_size = egui::vec2(140.0, 175.0);

    let (rect, response) = ui.allocate_exact_size(item_size, egui::Sense::click());

    if ui.is_rect_visible(rect) {
        let visuals = if is_selected {
            ui.visuals().widgets.active
        } else if response.hovered() {
            ui.visuals().widgets.hovered
        } else {
            ui.visuals().widgets.inactive
        };

        // Background
        ui.painter().rect_filled(
            rect,
            6,
            if is_selected {
                egui::Color32::from_rgb(60, 80, 120)
            } else {
                visuals.bg_fill
            },
        );

        // Thumbnail area
        let thumb_rect =
            egui::Rect::from_min_size(rect.min + egui::vec2(10.0, 10.0), egui::vec2(120.0, 90.0));

        match item.asset_type {
            AssetType::Images => {
                if let Some(texture) = thumbnail {
                    // Draw background for letterbox/pillarbox
                    ui.painter()
                        .rect_filled(thumb_rect, 4, egui::Color32::from_rgb(30, 30, 35));
                    // Fit thumbnail preserving aspect ratio, centered in thumb_rect
                    let tex_size = texture.size_vec2();
                    let scale =
                        (thumb_rect.width() / tex_size.x).min(thumb_rect.height() / tex_size.y);
                    let display_size = egui::vec2(tex_size.x * scale, tex_size.y * scale);
                    let centered_rect =
                        egui::Rect::from_center_size(thumb_rect.center(), display_size);
                    let image = egui::Image::new(texture)
                        .fit_to_exact_size(display_size)
                        .corner_radius(4);
                    image.paint_at(ui, centered_rect);
                } else if is_loading {
                    // Show loading placeholder with spinner
                    ui.painter()
                        .rect_filled(thumb_rect, 4, egui::Color32::from_rgb(45, 45, 50));
                    let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(thumb_rect));
                    child_ui.centered_and_justified(|ui| {
                        ui.spinner();
                    });
                } else {
                    // Failed to load — show image icon placeholder
                    ui.painter()
                        .rect_filled(thumb_rect, 4, egui::Color32::from_rgb(45, 45, 50));
                    ui.painter().text(
                        thumb_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        icons::IMAGE,
                        egui::FontId::proportional(32.0),
                        egui::Color32::from_white_alpha(100),
                    );
                }
            }
            AssetType::Models => {
                // Show model icon
                ui.painter()
                    .rect_filled(thumb_rect, 4, egui::Color32::from_rgb(40, 40, 45));
                ui.painter().text(
                    thumb_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    icons::CUBE,
                    egui::FontId::proportional(32.0),
                    egui::Color32::WHITE,
                );
            }
            AssetType::Textures => {
                // Show textures/palette icon
                ui.painter()
                    .rect_filled(thumb_rect, 4, egui::Color32::from_rgb(50, 40, 45));
                ui.painter().text(
                    thumb_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    icons::PALETTE,
                    egui::FontId::proportional(32.0),
                    egui::Color32::WHITE,
                );
            }
            AssetType::All => {}
        }

        // File info area
        let text_rect = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + 5.0, thumb_rect.max.y + 4.0),
            egui::pos2(rect.max.x - 5.0, rect.max.y - 5.0),
        );

        // Use galley for word-wrapped text
        let font_id = egui::FontId::proportional(10.0);
        let text_color = ui.visuals().text_color();
        let weak_text_color = ui.visuals().weak_text_color();
        let wrap_width = text_rect.width();

        // Create wrapped text for display name (prompt or filename), truncated to ~2 lines
        let display_name = item.display_name();
        let truncated_name = if display_name.chars().count() > 60 {
            let truncated: String = display_name.chars().take(57).collect();
            format!("{truncated}…")
        } else {
            display_name.to_string()
        };
        let galley = ui
            .painter()
            .layout(truncated_name, font_id.clone(), text_color, wrap_width);

        // Draw display name (may wrap to 2 lines)
        let mut text_pos = text_rect.left_top();
        ui.painter().galley(text_pos, galley.clone(), text_color);

        // If there's a subtitle, draw it below the title (truncated to fit card)
        if let Some(subtitle) = item.subtitle() {
            let truncated_sub = if subtitle.chars().count() > 30 {
                let t: String = subtitle.chars().take(27).collect();
                format!("{t}…")
            } else {
                subtitle
            };
            text_pos.y += galley.size().y + 2.0;
            ui.painter().text(
                text_pos,
                egui::Align2::LEFT_TOP,
                &truncated_sub,
                egui::FontId::proportional(9.0),
                weak_text_color,
            );
        }

        // Size and timestamp at bottom
        let info_text = item.formatted_size().to_string();
        ui.painter().text(
            text_rect.left_bottom(),
            egui::Align2::LEFT_BOTTOM,
            &info_text,
            egui::FontId::proportional(9.0),
            weak_text_color,
        );

        // Selection indicator
        if is_selected {
            ui.painter().circle_filled(
                rect.right_top() + egui::vec2(-12.0, 12.0),
                8.0,
                egui::Color32::from_rgb(80, 160, 80),
            );
            ui.painter().text(
                rect.right_top() + egui::vec2(-12.0, 12.0),
                egui::Align2::CENTER_CENTER,
                icons::CHECK,
                egui::FontId::proportional(10.0),
                egui::Color32::WHITE,
            );
        }
    }

    // Build hover tooltip lazily - only when mouse hovers
    response.on_hover_ui(|ui| {
        ui.set_max_width(300.0);

        // Custom name
        if let Some(custom_name) = &item.custom_name {
            ui.label(egui::RichText::new(format!("Name: {}", custom_name)).strong());
            ui.add_space(2.0);
        }

        // Prompt
        if let Some(prompt) = &item.prompt {
            ui.label(format!("Prompt: {}", prompt));
            ui.add_space(2.0);
        }

        // Model
        if let Some(model) = &item.model_name {
            ui.label(format!("Model: {}", model));
            ui.add_space(2.0);
        }

        // File info
        ui.separator();
        ui.label(egui::RichText::new(format!("File: {}", item.name)).secondary());
        ui.label(
            egui::RichText::new(format!("Created: {}", item.formatted_timestamp())).secondary(),
        );
        ui.label(egui::RichText::new(format!("Size: {}", item.formatted_size())).secondary());
    })
}
