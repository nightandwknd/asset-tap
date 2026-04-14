//! 3D model viewer using three-d with egui integration.
//!
//! Renders to an offscreen FBO (with depth buffer) then blits to egui's
//! framebuffer via PaintCallback — no GPU→CPU readback or texture re-upload.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

use eframe::{egui, egui_glow};
use three_d::*;

/// A renderable helper object (grid, axes) using unlit ColorMaterial.
type HelperObject = Gm<Mesh, ColorMaterial>;

/// Studio-lighting tuning for the procedural IBL environment.
///
/// Adjust these to make metallic PBR surfaces reflect a brighter or dimmer
/// "room". All values are sRGB byte triplets used to fill solid-color faces
/// of the environment cubemap; three-d's prefilter pipeline expands them
/// into proper irradiance and specular mip chains.
mod env_lighting {
    /// Top face — sky/ceiling. Brightest.
    pub const TOP: (u8, u8, u8) = (230, 230, 235);
    /// Side faces — walls. Medium brightness.
    pub const SIDES: (u8, u8, u8) = (200, 200, 205);
    /// Bottom face — floor. Dimmest.
    pub const BOTTOM: (u8, u8, u8) = (100, 100, 105);
    /// Ambient light intensity. Scales the overall environment contribution.
    pub const AMBIENT_INTENSITY: f32 = 0.3;
}

/// A renderable model object using physically-based material (affected by lights).
type ModelObject = Gm<Mesh, PhysicalMaterial>;

/// Camera state for orbit controls.
#[derive(Clone)]
pub struct CameraState {
    /// Horizontal angle (radians).
    pub theta: f32,
    /// Vertical angle (radians).
    pub phi: f32,
    /// Distance from target.
    pub distance: f32,
    /// Target point to orbit around.
    pub target: [f32; 3],
}

impl Default for CameraState {
    fn default() -> Self {
        Self {
            theta: 0.0,
            phi: 0.3,
            distance: 3.0,
            target: [0.0, 0.0, 0.0],
        }
    }
}

impl CameraState {
    /// Get the camera position from orbit parameters.
    /// Uses Y-up right-handed convention: theta rotates around Y axis, phi is elevation from horizontal.
    pub fn position(&self) -> Vec3 {
        let horizontal_dist = self.distance * self.phi.cos();
        let x = -horizontal_dist * self.theta.sin();
        let y = self.distance * self.phi.sin();
        let z = horizontal_dist * self.theta.cos();
        vec3(self.target[0] + x, self.target[1] + y, self.target[2] + z)
    }

    /// Apply drag rotation.
    pub fn rotate(&mut self, delta_x: f32, delta_y: f32) {
        self.theta += delta_x * 0.01;
        self.phi = (self.phi + delta_y * 0.01).clamp(-1.4, 1.4);
    }

    /// Apply zoom.
    pub fn zoom(&mut self, delta: f32) {
        self.distance = (self.distance * (1.0 - delta * 0.1)).clamp(0.5, 50.0);
    }

    /// Apply panning (moving the target point).
    pub fn pan(&mut self, delta_x: f32, delta_y: f32) {
        let pan_speed = self.distance * 0.005;
        let right_x = -self.theta.cos();
        let right_z = self.theta.sin();
        self.target[0] -= right_x * delta_x * pan_speed;
        self.target[2] -= right_z * delta_x * pan_speed;
        self.target[1] += delta_y * pan_speed;
    }

    /// Fit camera to bounding box.
    pub fn fit_to_bounds(&mut self, min: Vec3, max: Vec3) {
        self.target = [
            (min.x + max.x) / 2.0,
            (min.y + max.y) / 2.0,
            (min.z + max.z) / 2.0,
        ];
        let max_dim = (max.x - min.x).max(max.y - min.y).max(max.z - min.z);
        self.distance = max_dim * 2.0;
        self.phi = 0.3;
        self.theta = 0.0;
    }
}

/// Model metadata extracted from the file.
#[derive(Clone, Default)]
pub struct ModelInfo {
    pub file_size: u64,
    pub format: String,
    pub vertex_count: usize,
    pub triangle_count: usize,
}

impl ModelInfo {
    pub fn formatted_size(&self) -> String {
        if self.file_size < 1024 {
            format!("{} B", self.file_size)
        } else if self.file_size < 1024 * 1024 {
            format!("{:.1} KB", self.file_size as f64 / 1024.0)
        } else {
            format!("{:.1} MB", self.file_size as f64 / (1024.0 * 1024.0))
        }
    }
}

/// CPU model data loaded in background thread (before GPU upload).
struct CpuModelData {
    bounds_min: Vec3,
    bounds_max: Vec3,
    model_info: ModelInfo,
    cpu_model: three_d_asset::Model,
}

/// Final loaded model data (just bounds for camera fitting).
struct LoadedModelData {
    bounds_min: Vec3,
    bounds_max: Vec3,
}

/// Cached offscreen render target textures (color + depth).
/// Resized lazily when the viewport dimensions change.
struct OffscreenTargets {
    color: Texture2D,
    depth: DepthTexture2D,
    width: u32,
    height: u32,
}

/// 3D model viewer with orbit camera controls.
///
/// Renders to an offscreen FBO (with depth buffer), then blits to egui's
/// framebuffer via PaintCallback — no GPU→CPU pixel readback.
pub struct ModelViewer {
    /// Path to the currently loaded model.
    loaded_path: Option<PathBuf>,
    /// Model info (if loaded).
    pub model_info: Option<ModelInfo>,
    /// Camera state for orbit controls.
    pub camera_state: CameraState,
    /// Error message if loading failed.
    pub error: Option<String>,
    /// Loaded model bounds data.
    model_bounds: Option<LoadedModelData>,
    /// The three-d context (created once from glow).
    context: Option<Context>,
    /// The loaded GPU meshes with PhysicalMaterial (PBR, affected by lights).
    gpu_objects: Vec<ModelObject>,
    /// Whether to show the grid.
    pub show_grid: bool,
    /// Whether to show the axes.
    pub show_axes: bool,
    /// Cached grid GPU object (created once when model loads).
    cached_grid: Option<HelperObject>,
    /// Cached axes GPU objects (created once when model loads).
    cached_axes: Option<(HelperObject, HelperObject, HelperObject)>,
    /// Receiver for async loading results (CPU data only).
    loading_rx: Option<Receiver<Result<CpuModelData, String>>>,
    /// Whether a model is currently loading in the background.
    is_loading: bool,
    /// Cached offscreen color+depth textures for rendering with depth test.
    offscreen: Option<OffscreenTargets>,
    /// Cached ambient light with prefiltered environment. Required so
    /// metallic PBR surfaces have something to reflect (metals reflect their
    /// environment exclusively — no env map = near-black). Building the
    /// `AmbientLight::new_with_environment` involves GPU-side prefilter and
    /// irradiance convolution passes; caching the whole light (which owns the
    /// `Environment`) keeps per-frame cost at zero.
    cached_ambient: Option<AmbientLight>,
}

impl ModelViewer {
    pub fn new() -> Self {
        Self {
            loaded_path: None,
            model_info: None,
            camera_state: CameraState::default(),
            error: None,
            model_bounds: None,
            context: None,
            gpu_objects: Vec::new(),
            show_grid: true,
            show_axes: true,
            cached_grid: None,
            cached_axes: None,
            loading_rx: None,
            is_loading: false,
            offscreen: None,
            cached_ambient: None,
        }
    }

    /// Toggle axes visibility.
    pub fn toggle_axes(&mut self) {
        self.show_axes = !self.show_axes;
    }

    /// Toggle grid visibility.
    pub fn toggle_grid(&mut self) {
        self.show_grid = !self.show_grid;
    }

    pub fn loaded_path(&self) -> Option<&Path> {
        self.loaded_path.as_deref()
    }

    /// Initialize the three-d context from the glow context.
    pub fn init_context(&mut self, gl: Arc<glow::Context>) {
        if self.context.is_none() {
            // Log GL driver info for debugging texture issues across GPU vendors
            unsafe {
                use glow::HasContext;
                let renderer = gl.get_parameter_string(glow::RENDERER);
                let version = gl.get_parameter_string(glow::VERSION);
                let vendor = gl.get_parameter_string(glow::VENDOR);
                tracing::info!(
                    "GL context: vendor={}, renderer={}, version={}",
                    vendor,
                    renderer,
                    version
                );
            }
            self.context =
                Some(Context::from_gl_context(gl).expect("Failed to create three-d context"));
        }
    }

    /// Check if model is currently loading.
    pub fn is_loading(&self) -> bool {
        self.is_loading
    }

    /// Whether the viewer has a model ready to render.
    pub fn has_model(&self) -> bool {
        !self.gpu_objects.is_empty()
    }

    /// Start async loading of a model. Returns immediately and shows spinner.
    pub fn start_async_load(&mut self, path: PathBuf) {
        if self.loaded_path.as_deref() == Some(path.as_path()) && !self.gpu_objects.is_empty() {
            return;
        }

        if self.is_loading {
            self.loading_rx = None;
        }

        self.error = None;
        self.gpu_objects.clear();
        self.cached_grid = None;
        self.cached_axes = None;
        self.is_loading = true;

        let path_to_load = path.clone();
        self.loaded_path = Some(path);

        let (tx, rx) = channel();
        self.loading_rx = Some(rx);

        std::thread::spawn(move || {
            let result = Self::load_cpu_model_data(&path_to_load);
            let _ = tx.send(result);
        });
    }

    /// Poll for async loading completion. Call this every frame when is_loading() is true.
    pub fn poll_async_load(&mut self) {
        if !self.is_loading {
            return;
        }

        let rx = match self.loading_rx.as_ref() {
            Some(rx) => rx,
            None => {
                self.is_loading = false;
                return;
            }
        };

        match rx.try_recv() {
            Ok(Ok(cpu_data)) => match self.create_gpu_objects(cpu_data) {
                Ok(()) => {
                    self.is_loading = false;
                    self.loading_rx = None;
                }
                Err(e) => {
                    self.error = Some(e);
                    self.is_loading = false;
                    self.loading_rx = None;
                }
            },
            Ok(Err(err)) => {
                self.error = Some(err);
                self.is_loading = false;
                self.loading_rx = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.error = Some("Background loading thread disconnected".to_string());
                self.is_loading = false;
                self.loading_rx = None;
            }
        }
    }

    /// Create GPU objects from CPU data (must be called on main thread).
    fn create_gpu_objects(&mut self, cpu_data: CpuModelData) -> Result<(), String> {
        let context = self
            .context
            .as_ref()
            .ok_or("three-d context not initialized")?;

        let mut gpu_objects = Vec::new();

        for primitive in &cpu_data.cpu_model.geometries {
            if let three_d_asset::geometry::Geometry::Triangles(ref mesh) = primitive.geometry {
                let mut cpu_mesh = CpuMesh {
                    positions: Positions::F32(mesh.positions.to_f32()),
                    indices: mesh.indices.clone(),
                    normals: mesh.normals.clone(),
                    uvs: mesh.uvs.clone(),
                    tangents: mesh.tangents.clone(),
                    colors: mesh.colors.clone(),
                };

                let material_index = primitive.material_index.unwrap_or(0);
                let cpu_material = cpu_data.cpu_model.materials.get(material_index);

                // PhysicalMaterial's shader requires vertex tangents when a
                // normal map is present. Some GLBs (e.g., Meshy v6) ship normal
                // maps without precomputed tangents, which causes three-d's
                // shader link to fail. Compute them ourselves in that case.
                let needs_tangents = cpu_mesh.tangents.is_none()
                    && cpu_mesh.normals.is_some()
                    && cpu_mesh.uvs.is_some()
                    && cpu_material.is_some_and(|m| m.normal_texture.is_some());
                if needs_tangents {
                    cpu_mesh.compute_tangents();
                }

                let gpu_mesh = Mesh::new(context, &cpu_mesh);

                let physical_material = if let Some(mat) = cpu_material {
                    PhysicalMaterial::new_opaque(context, mat)
                } else {
                    PhysicalMaterial {
                        albedo: Srgba::new(180, 180, 180, 255),
                        metallic: 0.0,
                        roughness: 0.5,
                        ..Default::default()
                    }
                };

                let mut gm = Gm::new(gpu_mesh, physical_material);
                gm.set_transformation(primitive.transformation);
                gpu_objects.push(gm);
            }
        }

        self.model_info = Some(cpu_data.model_info);
        self.gpu_objects = gpu_objects;
        self.model_bounds = Some(LoadedModelData {
            bounds_min: cpu_data.bounds_min,
            bounds_max: cpu_data.bounds_max,
        });
        self.camera_state
            .fit_to_bounds(cpu_data.bounds_min, cpu_data.bounds_max);

        Ok(())
    }

    /// Load CPU model data on a background thread (blocking, no GPU operations).
    fn load_cpu_model_data(path: &Path) -> Result<CpuModelData, String> {
        let glb_data = match super::glb_webp::convert_webp_to_png(path) {
            Ok(data) => data,
            Err(e) => {
                return Err(format!(
                    "Failed to process GLB file: {}\n\
                    The file may be corrupted or use unsupported features.",
                    e
                ));
            }
        };

        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join(format!("viewer_{}.glb", std::process::id()));
        std::fs::write(&temp_path, &glb_data)
            .map_err(|e| format!("Failed to write temp file: {}", e))?;

        let cpu_model: CpuModel = {
            let load_result = three_d_asset::io::load(&[&temp_path]);

            let result = match load_result {
                Ok(mut loaded) => {
                    let temp_filename = temp_path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("viewer.glb");

                    match loaded.deserialize::<CpuModel>(temp_filename) {
                        Ok(model) => Ok(model),
                        Err(e) => Err(format!(
                            "Failed to deserialize model from {}\n\
                            Error: {}\n\n\
                            The file loaded but couldn't be deserialized. This may indicate:\n\
                            - Unsupported GLTF scene structure\n\
                            - Corrupted GLB file",
                            path.display(),
                            e
                        )),
                    }
                }
                Err(e) => Err(format!(
                    "Failed to load model file: {}\n\
                        The GLB file may be corrupted.",
                    e
                )),
            };

            let _ = std::fs::remove_file(&temp_path);
            result?
        };

        // Fix textures for cross-platform GPU compatibility:
        // - Convert RGB to RGBA (some drivers mishandle 3-byte-per-pixel alignment)
        let mut cpu_model = cpu_model;
        for material in &mut cpu_model.materials {
            fn fix_texture(texture: &mut Option<three_d_asset::Texture2D>) {
                if let Some(tex) = texture
                    && let three_d_asset::TextureData::RgbU8(data) = &tex.data
                {
                    let rgba: Vec<[u8; 4]> = data
                        .iter()
                        .map(|rgb| [rgb[0], rgb[1], rgb[2], 255])
                        .collect();
                    tex.data = three_d_asset::TextureData::RgbaU8(rgba);
                }
            }
            fix_texture(&mut material.albedo_texture);
            fix_texture(&mut material.emissive_texture);
            fix_texture(&mut material.normal_texture);
            fix_texture(&mut material.metallic_roughness_texture);
            fix_texture(&mut material.occlusion_texture);
            fix_texture(&mut material.occlusion_metallic_roughness_texture);
        }

        // Calculate bounds and counts
        let mut bounds_min = vec3(f32::MAX, f32::MAX, f32::MAX);
        let mut bounds_max = vec3(f32::MIN, f32::MIN, f32::MIN);
        let mut vertex_count = 0;
        let mut triangle_count = 0;

        for primitive in &cpu_model.geometries {
            if let three_d_asset::geometry::Geometry::Triangles(ref mesh) = primitive.geometry {
                vertex_count += mesh.vertex_count();
                triangle_count += mesh.triangle_count();

                let transform = primitive.transformation;
                for pos in &mesh.positions.to_f32() {
                    let world_pos = (transform * vec4(pos.x, pos.y, pos.z, 1.0)).truncate();
                    bounds_min.x = bounds_min.x.min(world_pos.x);
                    bounds_min.y = bounds_min.y.min(world_pos.y);
                    bounds_min.z = bounds_min.z.min(world_pos.z);
                    bounds_max.x = bounds_max.x.max(world_pos.x);
                    bounds_max.y = bounds_max.y.max(world_pos.y);
                    bounds_max.z = bounds_max.z.max(world_pos.z);
                }
            }
        }

        let metadata = std::fs::metadata(path).ok();
        let format = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("unknown")
            .to_uppercase();

        Ok(CpuModelData {
            bounds_min,
            bounds_max,
            model_info: ModelInfo {
                file_size: metadata.map(|m| m.len()).unwrap_or(0),
                format,
                vertex_count,
                triangle_count,
            },
            cpu_model,
        })
    }

    pub fn reset_camera(&mut self) {
        if let Some(ref bounds) = self.model_bounds {
            self.camera_state
                .fit_to_bounds(bounds.bounds_min, bounds.bounds_max);
        } else {
            self.camera_state = CameraState::default();
        }
    }

    /// Mark that the view needs to be re-rendered (e.g., after camera change).
    pub fn mark_dirty(&mut self) {
        // With PaintCallback, we re-render every frame the callback is painted.
        // This method is kept for API compatibility with input handlers.
    }

    /// Ensure offscreen color+depth textures exist at the given size.
    fn ensure_offscreen(&mut self, context: &Context, width: u32, height: u32) {
        let needs_resize = self
            .offscreen
            .as_ref()
            .is_none_or(|o| o.width != width || o.height != height);

        if needs_resize {
            let color = Texture2D::new_empty::<[u8; 4]>(
                context,
                width,
                height,
                Interpolation::Nearest,
                Interpolation::Nearest,
                None,
                Wrapping::ClampToEdge,
                Wrapping::ClampToEdge,
            );
            let depth = DepthTexture2D::new::<f32>(
                context,
                width,
                height,
                Wrapping::ClampToEdge,
                Wrapping::ClampToEdge,
            );
            self.offscreen = Some(OffscreenTargets {
                color,
                depth,
                width,
                height,
            });
        }
    }

    /// Render the 3D scene to an offscreen FBO then blit to egui's framebuffer.
    /// Called from inside PaintCallback where the GL context is current.
    ///
    /// `viewport_left` and `viewport_bottom` are the pixel offsets of the
    /// callback rect within the framebuffer (from left/bottom edges).
    fn render_direct(
        &mut self,
        width: u32,
        height: u32,
        viewport_left: i32,
        viewport_bottom: i32,
        egui_fbo: Option<glow::Framebuffer>,
    ) {
        let context = match self.context.as_ref() {
            Some(c) => c.clone(),
            None => return,
        };

        // Ensure offscreen targets exist at the right size
        self.ensure_offscreen(&context, width, height);
        let offscreen = self.offscreen.as_mut().unwrap();

        let eye = self.camera_state.position();
        let target = vec3(
            self.camera_state.target[0],
            self.camera_state.target[1],
            self.camera_state.target[2],
        );

        let camera = Camera::new_perspective(
            Viewport::new_at_origo(width, height),
            eye,
            target,
            vec3(0.0, 1.0, 0.0),
            degrees(45.0),
            0.1,
            1000.0,
        );

        // Render to our own FBO with proper color + depth attachments.
        // RenderTarget::new creates a new FBO internally and attaches the textures.
        let render_target = RenderTarget::new(
            offscreen.color.as_color_target(None),
            offscreen.depth.as_depth_target(),
        );

        render_target.clear(ClearState::color_and_depth(0.12, 0.12, 0.14, 1.0, 1.0));

        // Create and cache grid
        if self.cached_grid.is_none() {
            self.cached_grid = Some(Self::create_grid(&context, &self.model_bounds));
        }

        if self.show_grid
            && let Some(ref grid) = self.cached_grid
        {
            render_target.render(&camera, grid, &[]);
        }

        // Create and cache axes
        if self.cached_axes.is_none() {
            self.cached_axes = Some(Self::create_axes(&context, &self.model_bounds));
        }

        // Lazily build the ambient+environment once per viewer lifetime.
        // The Environment contained in AmbientLight requires GPU prefilter and
        // irradiance convolution, so this must not run every frame.
        if self.cached_ambient.is_none() {
            let env = Self::build_environment(&context);
            self.cached_ambient = Some(AmbientLight::new_with_environment(
                &context,
                env_lighting::AMBIENT_INTENSITY,
                Srgba::WHITE,
                &env,
            ));
        }
        let ambient = self
            .cached_ambient
            .as_ref()
            .expect("ambient just initialized");

        let directional = DirectionalLight::new(
            &context,
            2.0,
            Srgba::WHITE,
            vec3(-1.0, -1.0, -1.0).normalize(),
        );
        let fill = DirectionalLight::new(
            &context,
            0.5,
            Srgba::new(200, 210, 255, 255),
            vec3(1.0, 0.5, 0.5).normalize(),
        );
        let lights: Vec<&dyn Light> = vec![ambient, &directional, &fill];

        // Render model
        for obj in &self.gpu_objects {
            render_target.render(&camera, obj, &lights);
        }

        // Render axes on top
        if self.show_axes
            && let Some((ref x_axis, ref y_axis, ref z_axis)) = self.cached_axes
        {
            render_target.render(&camera, x_axis, &[]);
            render_target.render(&camera, y_axis, &[]);
            render_target.render(&camera, z_axis, &[]);
        }

        // Extract the FBO so Drop doesn't delete it — we need it for blit.
        let src_fbo = render_target.into_framebuffer();

        // Blit from our offscreen FBO to egui's framebuffer at the correct position.
        let gl: &glow::Context = &context;
        unsafe {
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, src_fbo);
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, egui_fbo);
            gl.disable(glow::SCISSOR_TEST);
            gl.blit_framebuffer(
                0,
                0,
                width as i32,
                height as i32,
                viewport_left,
                viewport_bottom,
                viewport_left + width as i32,
                viewport_bottom + height as i32,
                glow::COLOR_BUFFER_BIT,
                glow::NEAREST,
            );
            gl.enable(glow::SCISSOR_TEST);
            // Clean up the FBO (not the textures — those are owned by OffscreenTargets)
            if let Some(fbo) = src_fbo {
                gl.delete_framebuffer(fbo);
            }
            // Restore egui's FBO
            gl.bind_framebuffer(glow::FRAMEBUFFER, egui_fbo);
        }
    }

    /// Create an egui PaintCallback that renders the 3D scene.
    ///
    /// Renders three-d to an offscreen FBO (with depth), then blits to egui's
    /// framebuffer at the correct viewport offset — no GPU→CPU readback.
    pub fn paint_callback(viewer: &SharedModelViewer, rect: egui::Rect) -> egui::PaintCallback {
        let viewer = viewer.clone();

        let callback = egui_glow::CallbackFn::new(move |info, painter| {
            let vp = info.viewport_in_pixels();
            let width = vp.width_px as u32;
            let height = vp.height_px as u32;

            if width == 0 || height == 0 {
                return;
            }

            let mut viewer = viewer.lock().unwrap();
            if viewer.gpu_objects.is_empty() {
                return;
            }

            let egui_fbo = painter.intermediate_fbo();
            viewer.render_direct(width, height, vp.left_px, vp.from_bottom_px, egui_fbo);
        });

        egui::PaintCallback {
            rect,
            callback: Arc::new(callback),
        }
    }

    /// Build a simple procedural environment cubemap. Gives metallic PBR
    /// surfaces something to reflect — without this, metals render near-black
    /// because they have no diffuse albedo and rely entirely on env reflection.
    ///
    /// Uses a neutral studio-style gradient: brighter on top (sky), mid on
    /// sides, dimmer on bottom (ground). All faces are solid-color 1x1 textures
    /// — three-d's prefilter pipeline expands these into proper irradiance and
    /// specular mip chains.
    fn build_environment(context: &Context) -> TextureCubeMap {
        fn face(name: &str, rgb: (u8, u8, u8)) -> CpuTexture {
            CpuTexture {
                name: name.to_string(),
                data: TextureData::RgbaU8(vec![[rgb.0, rgb.1, rgb.2, 255]]),
                width: 1,
                height: 1,
                mipmap: None,
                ..Default::default()
            }
        }
        let top = face("env_top", env_lighting::TOP);
        let right = face("env_right", env_lighting::SIDES);
        let left = face("env_left", env_lighting::SIDES);
        let front = face("env_front", env_lighting::SIDES);
        let back = face("env_back", env_lighting::SIDES);
        let bottom = face("env_bottom", env_lighting::BOTTOM);
        TextureCubeMap::new(context, &right, &left, &top, &bottom, &front, &back)
    }

    /// Create the floor grid mesh.
    fn create_grid(context: &Context, model_bounds: &Option<LoadedModelData>) -> HelperObject {
        let (grid_size, grid_y, center_x, center_z) = if let Some(bounds) = model_bounds {
            let max_extent = (bounds.bounds_max.x - bounds.bounds_min.x)
                .max(bounds.bounds_max.y - bounds.bounds_min.y)
                .max(bounds.bounds_max.z - bounds.bounds_min.z);
            let size = max_extent * 2.5;
            let cx = (bounds.bounds_min.x + bounds.bounds_max.x) / 2.0;
            let cz = (bounds.bounds_min.z + bounds.bounds_max.z) / 2.0;
            let floor_y = bounds.bounds_min.y;
            (size, floor_y, cx, cz)
        } else {
            (2.0, 0.0, 0.0, 0.0)
        };

        let grid_lines = 20i32;
        let half_size = grid_size / 2.0;
        let step = grid_size / grid_lines as f32;
        let line_thickness = step * 0.02;

        let mut positions: Vec<Vec3> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();

        let mut add_line_quad = |x0: f32, z0: f32, x1: f32, z1: f32| {
            let base = positions.len() as u32;
            let dx = x1 - x0;
            let dz = z1 - z0;
            let len = (dx * dx + dz * dz).sqrt();
            let px = -dz / len * line_thickness;
            let pz = dx / len * line_thickness;

            positions.push(vec3(x0 - px, grid_y, z0 - pz));
            positions.push(vec3(x0 + px, grid_y, z0 + pz));
            positions.push(vec3(x1 + px, grid_y, z1 + pz));
            positions.push(vec3(x1 - px, grid_y, z1 - pz));

            indices.extend_from_slice(&[base, base + 1, base + 2]);
            indices.extend_from_slice(&[base, base + 2, base + 3]);
        };

        for i in 0..=grid_lines {
            let z = center_z - half_size + i as f32 * step;
            add_line_quad(center_x - half_size, z, center_x + half_size, z);
        }
        for i in 0..=grid_lines {
            let x = center_x - half_size + i as f32 * step;
            add_line_quad(x, center_z - half_size, x, center_z + half_size);
        }

        Gm::new(
            Mesh::new(
                context,
                &CpuMesh {
                    positions: Positions::F32(positions),
                    indices: Indices::U32(indices),
                    ..Default::default()
                },
            ),
            ColorMaterial {
                color: Srgba::new(55, 55, 60, 255),
                render_states: RenderStates {
                    write_mask: WriteMask::COLOR,
                    depth_test: DepthTest::LessOrEqual,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
    }

    /// Create the XYZ axes with arrows.
    fn create_axes(
        context: &Context,
        model_bounds: &Option<LoadedModelData>,
    ) -> (HelperObject, HelperObject, HelperObject) {
        let (grid_size, origin) = if let Some(bounds) = model_bounds {
            let max_extent = (bounds.bounds_max.x - bounds.bounds_min.x)
                .max(bounds.bounds_max.y - bounds.bounds_min.y)
                .max(bounds.bounds_max.z - bounds.bounds_min.z);
            let cx = (bounds.bounds_min.x + bounds.bounds_max.x) / 2.0;
            let cy = (bounds.bounds_min.y + bounds.bounds_max.y) / 2.0;
            let cz = (bounds.bounds_min.z + bounds.bounds_max.z) / 2.0;
            (max_extent * 2.5, vec3(cx, cy, cz))
        } else {
            (2.0, vec3(0.0, 0.0, 0.0))
        };

        let axes_length = grid_size * 0.24;
        let axes_thickness = axes_length * 0.0075;
        let arrow_size = axes_length * 0.04;

        let create_axis_with_arrow = |dir: Vec3, color: Srgba| {
            let half_thick = axes_thickness / 2.0;
            let shaft_length = axes_length - arrow_size;

            let up_hint = if dir.y.abs() > 0.9 {
                vec3(1.0, 0.0, 0.0)
            } else {
                vec3(0.0, 1.0, 0.0)
            };
            let perp1 = dir.cross(up_hint).normalize();
            let perp2 = dir.cross(perp1).normalize();

            let mut positions: Vec<Vec3> = Vec::new();
            let mut indices: Vec<u32> = Vec::new();

            // Shaft
            let p1 = perp1 * half_thick;
            let p2 = perp2 * half_thick;
            let shaft_end = dir * shaft_length;

            let base = positions.len() as u32;
            positions.extend_from_slice(&[
                origin - p1 - p2,
                origin + p1 - p2,
                origin + p1 + p2,
                origin - p1 + p2,
                origin + shaft_end - p1 - p2,
                origin + shaft_end + p1 - p2,
                origin + shaft_end + p1 + p2,
                origin + shaft_end - p1 + p2,
            ]);
            #[rustfmt::skip]
            indices.extend_from_slice(&[
                base, base+2, base+1, base, base+3, base+2,
                base+4, base+5, base+6, base+4, base+6, base+7,
                base, base+1, base+5, base, base+5, base+4,
                base+2, base+3, base+7, base+2, base+7, base+6,
                base, base+4, base+7, base, base+7, base+3,
                base+1, base+2, base+6, base+1, base+6, base+5,
            ]);

            // Arrowhead
            let ab = arrow_size * 0.5;
            let a1 = perp1 * ab;
            let a2 = perp2 * ab;
            let tip = origin + dir * axes_length;
            let arrow_base = positions.len() as u32;
            positions.extend_from_slice(&[
                tip,
                origin + shaft_end - a1 - a2,
                origin + shaft_end + a1 - a2,
                origin + shaft_end + a1 + a2,
                origin + shaft_end - a1 + a2,
            ]);
            #[rustfmt::skip]
            indices.extend_from_slice(&[
                arrow_base, arrow_base+1, arrow_base+2,
                arrow_base, arrow_base+2, arrow_base+3,
                arrow_base, arrow_base+3, arrow_base+4,
                arrow_base, arrow_base+4, arrow_base+1,
                arrow_base+1, arrow_base+3, arrow_base+2,
                arrow_base+1, arrow_base+4, arrow_base+3,
            ]);

            Gm::new(
                Mesh::new(
                    context,
                    &CpuMesh {
                        positions: Positions::F32(positions),
                        indices: Indices::U32(indices),
                        ..Default::default()
                    },
                ),
                ColorMaterial {
                    color,
                    render_states: RenderStates {
                        depth_test: DepthTest::Always,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
        };

        (
            create_axis_with_arrow(vec3(-1.0, 0.0, 0.0), Srgba::new(220, 60, 60, 255)),
            create_axis_with_arrow(vec3(0.0, 1.0, 0.0), Srgba::new(60, 220, 60, 255)),
            create_axis_with_arrow(vec3(0.0, 0.0, 1.0), Srgba::new(60, 100, 220, 255)),
        )
    }
}

impl Default for ModelViewer {
    fn default() -> Self {
        Self::new()
    }
}

/// Wrapper for sharing ModelViewer across threads.
pub type SharedModelViewer = Arc<Mutex<ModelViewer>>;
