//! SPIKE web harness. See spike/browser-viewer/README.md and FEASIBILITY.md.
//!
//! SHARED SOURCE (compiled from the repo, not copied by hand):
//! * `viewer::model` is `gui/src/viewer/model.rs` as adapted by build.rs: 4 leading `//!` lines
//!   turned into `//`, plus exactly two asserted wasm32 patches (path loader call; `SendWrapper`
//!   around the shared viewer). Everything else is the desktop code: ModelViewer, the three-d
//!   PhysicalMaterial/IBL renderer, camera, `tick`, CPU `SkinnedClip` playback upload,
//!   `paint_callback`.
//! * `asset_tap_core::{SkinnedClip, BindMarker}` are the real `core/src/rig` files via the shim.
//!
//! STAND-IN / NEW CODE (written for the spike, marked where it appears):
//! * `glb_webp` stub, `bytes_ext` (bytes -> CpuModel -> GPU; the seam a future viewer crate must
//!   own), and the eframe web shell + status reporting.
#![allow(dead_code, unused_imports, clippy::all)]

use std::{cell::RefCell, rc::Rc, sync::{Arc, Mutex}};
use wasm_bindgen::prelude::*;

mod viewer {
    /// STAND-IN: the real one reads a file path (WebP->PNG GLB texture rewrite).
    /// Never called on the bytes path; WebP-in-GLB is therefore NOT handled here.
    pub mod glb_webp {
        pub fn convert_webp_to_png(_p: &std::path::Path) -> Result<Vec<u8>, String> {
            Err("stand-in: path-based glb_webp unavailable on wasm".into())
        }
    }
    pub mod model {
        include!(concat!(env!("OUT_DIR"), "/viewer_model.rs"));

        /// NEW CODE (not in the desktop viewer): the bytes entry the desktop path lacks.
        /// Duplicates the texture-fix + bounds logic of `load_cpu_model_data` (private,
        /// path/thread/temp-file based) because that fn cannot be reused as-is.
        pub mod bytes_ext {
            use super::*;
            pub struct LoadReport { pub parse_ms: f64, pub gpu_ms: f64, pub verts: usize, pub tris: usize, pub has_clip: bool, pub clip_s: f32 }
            impl ModelViewer {
                pub fn load_glb_bytes(&mut self, bytes: &[u8], now: &dyn Fn() -> f64) -> Result<LoadReport, String> {
                    let t0 = now();
                    let mut raw = three_d_asset::io::RawAssets::new();
                    raw.insert("model.glb", bytes.to_vec());
                    let mut cpu_model: CpuModel = raw.deserialize("model.glb").map_err(|e| format!("deserialize: {e}"))?;
                    for m in &mut cpu_model.materials {
                        fn fix(t: &mut Option<three_d_asset::Texture2D>) {
                            if let Some(tex) = t && let three_d_asset::TextureData::RgbU8(d) = &tex.data {
                                tex.data = three_d_asset::TextureData::RgbaU8(d.iter().map(|c| [c[0], c[1], c[2], 255]).collect());
                            }
                        }
                        fix(&mut m.albedo_texture); fix(&mut m.emissive_texture); fix(&mut m.normal_texture);
                        fix(&mut m.metallic_roughness_texture); fix(&mut m.occlusion_texture);
                        fix(&mut m.occlusion_metallic_roughness_texture);
                    }
                    let mut bmin = vec3(f32::MAX, f32::MAX, f32::MAX);
                    let mut bmax = vec3(f32::MIN, f32::MIN, f32::MIN);
                    let (mut verts, mut tris) = (0, 0);
                    for p in &cpu_model.geometries {
                        if let three_d_asset::geometry::Geometry::Triangles(ref m) = p.geometry {
                            verts += m.vertex_count(); tris += m.triangle_count();
                            for q in &m.positions.to_f32() {
                                let w = (p.transformation * vec4(q.x, q.y, q.z, 1.0)).truncate();
                                bmin = vec3(bmin.x.min(w.x), bmin.y.min(w.y), bmin.z.min(w.z));
                                bmax = vec3(bmax.x.max(w.x), bmax.y.max(w.y), bmax.z.max(w.z));
                            }
                        }
                    }
                    // REAL rig code, from bytes: same steps as skeleton::read_gltf + import_buffers.
                    let clip = gltf::Gltf::from_slice_without_validation(bytes).ok().and_then(|g| {
                        let gltf::Gltf { document, blob } = g;
                        let buffers = gltf::import_buffers(&document, None, blob).ok()?;
                        if document.skins().next().is_none() { return None; }
                        asset_tap_core::SkinnedClip::from_gltf(&document, &buffers).ok()
                    });
                    let (has_clip, clip_s) = clip.as_ref().map(|c| (c.has_animation(), c.duration)).unwrap_or((false, 0.0));
                    let t1 = now();
                    self.create_gpu_objects(CpuModelData {
                        bounds_min: bmin, bounds_max: bmax,
                        model_info: ModelInfo { file_size: bytes.len() as u64, format: "GLB".into(), vertex_count: verts, triangle_count: tris },
                        cpu_model, clip,
                    })?;
                    if has_clip { self.set_playing(true); }
                    Ok(LoadReport { parse_ms: t1 - t0, gpu_ms: now() - t1, verts, tris, has_clip, clip_s })
                }
            }
        }
    }
}
use viewer::model::{ModelViewer, SharedModelViewer};

fn now_ms() -> f64 {
    web_sys::window().and_then(|w| w.performance()).map(|p| p.now()).unwrap_or(0.0)
}

/// Machine-readable status for whoever drives the page: `window.__viewerStatus`.
fn publish(status: &str) {
    if let Some(w) = web_sys::window() {
        let _ = js_sys::Reflect::set(&w, &"__viewerStatus".into(), &JsValue::from_str(status));
    }
}

struct HarnessApp {
    viewer: SharedModelViewer,
    gl: Option<Arc<glow::Context>>,
    inbox: Rc<RefCell<Option<Result<Vec<u8>, String>>>>,
    status: serde_lite::Status,
}

mod serde_lite {
    #[derive(Default, Clone)]
    pub struct Status {
        pub loaded: bool, pub error: Option<String>, pub verts: usize, pub tris: usize,
        pub has_clip: bool, pub clip_s: f32, pub parse_ms: f64, pub gpu_ms: f64, pub bytes: usize,
        pub frames: u64, pub gl: String, pub rayon_probe: String, pub src: String,
    }
    impl Status {
        pub fn json(&self, playing: bool, t: f32) -> String {
            let e = self.error.as_deref().map(|s| format!("{:?}", s)).unwrap_or("null".into());
            format!(
                "{{\"loaded\":{},\"error\":{},\"src\":{:?},\"bytes\":{},\"verts\":{},\"tris\":{},\"hasClip\":{},\"clipSeconds\":{:.3},\"playing\":{},\"playTime\":{:.3},\"parseMs\":{:.1},\"gpuUploadMs\":{:.1},\"frames\":{},\"gl\":{:?},\"rayonProbe\":{:?}}}",
                self.loaded, e, self.src, self.bytes, self.verts, self.tris, self.has_clip, self.clip_s, playing, t, self.parse_ms, self.gpu_ms, self.frames, self.gl, self.rayon_probe)
        }
    }
}

impl HarnessApp {
    fn ingest(&mut self, bytes: Vec<u8>, src: &str) {
        self.status.src = src.into();
        self.status.bytes = bytes.len();
        let mut v = self.viewer.lock().unwrap();
        if let Some(gl) = &self.gl { v.init_context(gl.clone()); }
        match v.load_glb_bytes(&bytes, &now_ms) {
            Ok(r) => {
                let s = &mut self.status;
                s.loaded = true; s.error = None; s.verts = r.verts; s.tris = r.tris;
                s.has_clip = r.has_clip; s.clip_s = r.clip_s; s.parse_ms = r.parse_ms; s.gpu_ms = r.gpu_ms;
            }
            Err(e) => { self.status.loaded = false; self.status.error = Some(e); }
        }
    }
}

impl eframe::App for HarnessApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let pending = self.inbox.borrow_mut().take();
        if let Some(res) = pending {
            match res { Ok(b) => self.ingest(b, "fetch"), Err(e) => self.status.error = Some(e) }
        }
        let dropped: Vec<_> = ctx.input(|i| i.raw.dropped_files.clone());
        for f in dropped { if let Some(b) = f.bytes { self.ingest(b.to_vec(), "drop"); } }

        let dt = ctx.input(|i| i.stable_dt);
        let (playing, t) = {
            let mut v = self.viewer.lock().unwrap();
            v.tick(dt);
            (v.is_playing(), v.play_time().0)
        };
        egui::Panel::top("bar").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button(if playing { "Pause" } else { "Play" }).clicked() { self.viewer.lock().unwrap().toggle_playing(); }
                if ui.button("Reset camera").clicked() { self.viewer.lock().unwrap().reset_camera(); }
                let s = &self.status;
                ui.label(format!("{} | {} verts, {} tris | clip {:.2}s t={:.2} | parse {:.0} ms, GPU upload {:.0} ms | {}",
                    if s.loaded { "loaded" } else { "waiting (drop a .glb or use ?glb=URL)" }, s.verts, s.tris, s.clip_s, t, s.parse_ms, s.gpu_ms,
                    s.error.clone().unwrap_or_default()));
            });
        });
        egui::CentralPanel::default().show_inside(ui, |ui| {
            let (rect, resp) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
            if self.status.loaded {
                let mut v = self.viewer.lock().unwrap();
                if resp.dragged() { let d = resp.drag_delta(); v.camera_state.rotate(d.x, d.y); }
                let sc = ui.input(|i| i.smooth_scroll_delta.y);
                if resp.hovered() && sc != 0.0 { v.camera_state.zoom(sc); }
                drop(v);
                ui.painter().add(ModelViewer::paint_callback(&self.viewer, rect));
            }
        });
        self.status.frames += 1;
        let (playing, t) = { let v = self.viewer.lock().unwrap(); (v.is_playing(), v.play_time().0) };
        publish(&self.status.json(playing, t));
        ctx.request_repaint();
    }
}

async fn fetch_bytes(url: String) -> Result<Vec<u8>, String> {
    use wasm_bindgen::JsCast;
    let w = web_sys::window().ok_or("no window")?;
    let resp: web_sys::Response = wasm_bindgen_futures::JsFuture::from(w.fetch_with_str(&url)).await
        .map_err(|e| format!("fetch {url}: {e:?}"))?.dyn_into().map_err(|_| "not a Response")?;
    if !resp.ok() { return Err(format!("fetch {url}: HTTP {}", resp.status())); }
    let buf = wasm_bindgen_futures::JsFuture::from(resp.array_buffer().map_err(|e| format!("{e:?}"))?).await
        .map_err(|e| format!("{e:?}"))?;
    Ok(js_sys::Uint8Array::new(&buf).to_vec())
}

/// Called from index.html. `glb_url` may be empty (then drag a GLB onto the canvas).
#[wasm_bindgen]
pub async fn start(canvas_id: String, glb_url: String) -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    use wasm_bindgen::JsCast;
    let canvas = web_sys::window().unwrap().document().unwrap().get_element_by_id(&canvas_id)
        .ok_or("canvas not found")?.dyn_into::<web_sys::HtmlCanvasElement>()?;
    let inbox: Rc<RefCell<Option<Result<Vec<u8>, String>>>> = Rc::default();
    if !glb_url.is_empty() {
        let inbox = inbox.clone();
        wasm_bindgen_futures::spawn_local(async move { *inbox.borrow_mut() = Some(fetch_bytes(glb_url).await); });
    }
    // Does rayon's global pool survive on wasm32-unknown-unknown without threads?
    let rayon_probe = std::panic::catch_unwind(|| {
        use rayon::prelude::*;
        let s: u64 = (0..10_000u64).into_par_iter().map(|x| x * 2).sum();
        format!("par_iter ok sum={s} current_num_threads={}", rayon::current_num_threads())
    }).unwrap_or_else(|_| "par_iter PANICKED".into());
    eframe::WebRunner::new().start(canvas, eframe::WebOptions::default(), Box::new(move |cc| {
        let gl = cc.gl.clone();
        let mut status = serde_lite::Status::default();
        status.rayon_probe = rayon_probe;
        if let Some(gl) = &gl {
            use glow::HasContext;
            status.gl = unsafe { format!("{} | {} | {}", gl.get_parameter_string(glow::VERSION), gl.get_parameter_string(glow::RENDERER), gl.get_parameter_string(glow::VENDOR)) };
        } else { status.gl = "NO GL CONTEXT".into(); }
        Ok(Box::new(HarnessApp { viewer: Arc::new(Mutex::new(send_wrapper::SendWrapper::new(ModelViewer::new()))), gl, inbox, status }))
    })).await
}
