//! Background texture thumbnail loading and caching.
//!
//! This module provides asynchronous thumbnail loading to prevent UI freezes
//! when displaying texture previews. Uses a thread pool for parallel loading.

use eframe::egui;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;

/// Number of parallel thumbnail loader threads
const NUM_LOADER_THREADS: usize = 4;

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

/// Cache for texture thumbnails with background loading.
pub struct TextureCache {
    /// Sender to request thumbnail loading
    thumb_tx: Sender<PathBuf>,
    /// Receiver for loaded thumbnails
    thumb_rx: Receiver<ThumbnailMessage>,
    /// Cache of loaded thumbnail texture handles
    cache: HashMap<PathBuf, egui::TextureHandle>,
    /// Set of paths that have been requested for loading
    loading_requested: HashSet<PathBuf>,
    /// Paths that failed to load
    failed: HashSet<PathBuf>,
    /// Current textures directory being displayed
    current_dir: Option<PathBuf>,
}

impl Default for TextureCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Load a single thumbnail - used by worker threads
fn load_thumbnail(path: &PathBuf, thumb_size: u32) -> Result<ThumbnailData, image::ImageError> {
    // Use ImageReader with format guessing to handle mislabeled files
    // (e.g., WebP files with .png extension from FBX extraction)
    let reader = image::ImageReader::open(path)
        .map_err(image::ImageError::IoError)?
        .with_guessed_format()
        .map_err(image::ImageError::IoError)?;

    let img = reader.decode()?;

    // Use thumbnail() for fast downscaling
    let thumb = img.thumbnail(thumb_size, thumb_size);
    let rgba = thumb.to_rgba8();
    let (width, height) = rgba.dimensions();

    Ok(ThumbnailData {
        pixels: rgba.into_raw(),
        width,
        height,
    })
}

impl TextureCache {
    /// Create a new texture cache with background loader thread pool.
    pub fn new() -> Self {
        let (request_tx, request_rx) = channel::<PathBuf>();
        let (result_tx, result_rx) = channel::<ThumbnailMessage>();

        // Wrap the receiver in Arc<Mutex> so multiple threads can pull from it
        let request_rx = Arc::new(Mutex::new(request_rx));

        // Spawn a pool of thumbnail loader threads for parallel processing
        for i in 0..NUM_LOADER_THREADS {
            let rx = Arc::clone(&request_rx);
            let tx = result_tx.clone();

            thread::spawn(move || {
                loop {
                    // Try to get a path to process
                    let path = {
                        let receiver = rx.lock().unwrap();
                        receiver.recv()
                    };

                    match path {
                        Ok(path) => {
                            // Load at 256px for quality when scaled up
                            match load_thumbnail(&path, 256) {
                                Ok(data) => {
                                    let _ = tx.send(ThumbnailMessage::Loaded(path, data));
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "Worker {}: Failed to load {:?}: {}",
                                        i,
                                        path,
                                        e
                                    );
                                    let _ = tx.send(ThumbnailMessage::Failed(path));
                                }
                            }
                        }
                        Err(_) => {
                            // Channel closed, exit thread
                            break;
                        }
                    }
                }
            });
        }

        Self {
            thumb_tx: request_tx,
            thumb_rx: result_rx,
            cache: HashMap::new(),
            loading_requested: HashSet::new(),
            failed: HashSet::new(),
            current_dir: None,
        }
    }

    /// Set the current textures directory. Clears cache if directory changed.
    pub fn set_directory(&mut self, dir: Option<&PathBuf>) {
        if self.current_dir.as_ref() != dir {
            self.current_dir = dir.cloned();
            // Clear cache when directory changes
            self.cache.clear();
            self.loading_requested.clear();
            self.failed.clear();
        }
    }

    /// Process any loaded thumbnails from background thread.
    /// Returns true if any thumbnails were processed (caller should request repaint).
    pub fn process_loaded(&mut self, ctx: &egui::Context) -> bool {
        let mut processed_any = false;

        while let Ok(msg) = self.thumb_rx.try_recv() {
            processed_any = true;
            match msg {
                ThumbnailMessage::Loaded(path, data) => {
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [data.width as usize, data.height as usize],
                        &data.pixels,
                    );
                    let texture = ctx.load_texture(
                        path.to_string_lossy(),
                        image,
                        egui::TextureOptions::LINEAR, // Use linear filtering for smooth scaling
                    );
                    self.cache.insert(path, texture);
                }
                ThumbnailMessage::Failed(path) => {
                    self.failed.insert(path);
                }
            }
        }

        processed_any
    }

    /// Get a thumbnail for the given path, requesting load if not cached.
    /// Returns None if still loading or failed.
    pub fn get_thumbnail(&mut self, path: &PathBuf) -> Option<&egui::TextureHandle> {
        // Check cache first
        if self.cache.contains_key(path) {
            return self.cache.get(path);
        }

        // Check if already failed
        if self.failed.contains(path) {
            return None;
        }

        // Request loading if not already requested
        if !self.loading_requested.contains(path) {
            self.loading_requested.insert(path.clone());
            let _ = self.thumb_tx.send(path.clone());
        }

        None
    }

    /// Invalidate a specific path from the cache so it will be re-loaded.
    /// Used when the image file at this path has been replaced (e.g., regeneration).
    pub fn invalidate(&mut self, path: &PathBuf) {
        self.cache.remove(path);
        self.loading_requested.remove(path);
        self.failed.remove(path);
    }

    /// Check if there are any thumbnails still loading.
    pub fn has_pending(&self) -> bool {
        self.loading_requested.len() > self.cache.len() + self.failed.len()
    }
}
